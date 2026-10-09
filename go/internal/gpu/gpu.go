// Package gpu is Hover's own small binding to wgpu-native v29: the calls the office
// renderer and the notch spike need, and nothing else. go-webgpu was dropped for it (its
// v0.5.5 structs no longer matched wgpu-native v29, see docs/development/go-port.md).
//
// Every C struct is in wire.go, and wire_test.go checks their sizes, offsets and the
// enum numbers against what the C compiler gave for wgpu-native's own webgpu.h. The
// public API takes Go values; each call builds its wire structs, pins them (so the GC
// can neither move nor free what the C side reads) and unpins them when the call returns.
//
// Windows calls wgpu_native.dll through syscall (no C compiler). Linux and macOS go
// through goffi (no C compiler either).
//
// ponytail: only what the office and the spike draw with. A new call is one method here
// and, if it takes a struct, one wire struct and one line in wire_test.go.
package gpu

import (
	"errors"
	"fmt"
	"runtime"
	"sync"
	"unsafe"
)

// ---- the public enums (webgpu.h v29's numbers) --------------------------------------

type (
	TextureFormat uint32
	TextureUsage  uint64
	BufferUsage   uint64
	ShaderStage   uint64
	SampleType    uint32
	IndexFormat   uint32
	VertexFormat  uint32
	Topology      uint32
	FrontFace     uint32
	CullMode      uint32
	Compare       uint32
	BlendOp       uint32
	BlendFactor   uint32
	FilterMode    uint32
	AddressMode   uint32
	BackendType   uint32
	AdapterType   uint32
)

const (
	FormatRGBA8Unorm     TextureFormat = 0x16
	FormatRGBA8UnormSrgb TextureFormat = 0x17
	FormatRGBA16Float    TextureFormat = 0x28
	FormatDepth32Float   TextureFormat = 0x30

	TextureCopySrc        TextureUsage = 0x1
	TextureCopyDst        TextureUsage = 0x2
	TextureBinding        TextureUsage = 0x4
	TextureRenderAttachmt TextureUsage = 0x10

	BufferMapRead BufferUsage = 0x1
	BufferCopySrc BufferUsage = 0x4
	BufferCopyDst BufferUsage = 0x8
	BufferIndex   BufferUsage = 0x10
	BufferVertex  BufferUsage = 0x20
	BufferUniform BufferUsage = 0x40

	StageVertex   ShaderStage = 0x1
	StageFragment ShaderStage = 0x2

	SampleFloat SampleType = 2
	SampleDepth SampleType = 4

	IndexUint32 IndexFormat = 2

	VertexFloat32x2 VertexFormat = 0x1d
	VertexFloat32x3 VertexFormat = 0x1e

	PointList    Topology = 1
	TriangleList Topology = 4

	FrontCCW FrontFace = 1

	CullNone  CullMode = 1
	CullFront CullMode = 2
	CullBack  CullMode = 3

	CompareLessEqual Compare = 4

	BlendAdd BlendOp = 1

	FactorOne              BlendFactor = 2
	FactorSrcAlpha         BlendFactor = 5
	FactorOneMinusSrcAlpha BlendFactor = 6

	FilterNearest FilterMode = 1
	FilterLinear  FilterMode = 2

	ClampToEdge AddressMode = 1

	BackendD3D12    BackendType = 4
	BackendMetal    BackendType = 5
	BackendVulkan   BackendType = 6
	BackendOpenGL   BackendType = 7
	BackendOpenGLES BackendType = 8
	AdapterCPU      AdapterType = 3
)

// ---- pinning and strings ------------------------------------------------------------

// pins keeps what one call hands to wgpu-native in place until the call returns.
type pins struct{ p runtime.Pinner }

// ptr pins *v and returns its address.
func ptr[T any](k *pins, v *T) uintptr {
	if v == nil {
		return 0
	}
	k.p.Pin(v)
	return uintptr(unsafe.Pointer(v))
}

// slice pins a slice's backing array and returns its address (0 for an empty slice).
func slice[T any](k *pins, s []T) uintptr {
	if len(s) == 0 {
		return 0
	}
	return ptr(k, &s[0])
}

// str is a Go string as a webgpu.h string view, its bytes pinned.
func (k *pins) str(s string) stringView {
	if s == "" {
		return noString
	}
	b := []byte(s)
	return stringView{slice(k, b), uintptr(len(b))}
}

func (k *pins) done() { k.p.Unpin() }

// goString copies a string view out of wgpu-native's memory.
func goString(v stringView) string {
	if v.data == 0 || v.length == 0 || v.length == strlen {
		return ""
	}
	return string(unsafe.Slice(cPtr[byte](v.data), v.length))
}

// cPtr turns an address wgpu-native gave back into a Go pointer. It reads the uintptr's
// bits as a pointer (what x/sys does) rather than converting, which go vet would flag:
// the memory is wgpu-native's, never the Go heap's, so the GC has nothing to track.
func cPtr[T any](a uintptr) *T { return *(**T)(unsafe.Pointer(&a)) }

// ---- the library and the callbacks ---------------------------------------------------

var (
	initOnce sync.Once
	initErr  error
)

// Init loads wgpu-native (wgpu_native.dll beside the exe or at WGPU_NATIVE_PATH on
// Windows, libwgpu_native.so / .dylib elsewhere) and makes the callbacks. Later calls
// return the first result.
func Init() error {
	initOnce.Do(func() { initErr = load() })
	return initErr
}

// pending is one asynchronous request: the callback writes it, the caller reads it. The
// callback gets its id, never a Go pointer.
type pending struct {
	done   bool
	status uint32
	handle uintptr
	typ    uint32
	msg    string
}

var requests struct {
	sync.Mutex
	next uintptr
	m    map[uintptr]*pending
}

func newRequest() (uintptr, *pending) {
	requests.Lock()
	defer requests.Unlock()
	if requests.m == nil {
		requests.m = map[uintptr]*pending{}
	}
	requests.next++
	p := &pending{}
	requests.m[requests.next] = p
	return requests.next, p
}

func takeRequest(id uintptr) {
	requests.Lock()
	delete(requests.m, id)
	requests.Unlock()
}

// complete is what every callback does with what wgpu-native passed.
func complete(id uintptr, status uint32, handle uintptr, typ uint32, msg string) {
	requests.Lock()
	p := requests.m[id]
	requests.Unlock()
	if p == nil {
		return
	}
	p.status, p.handle, p.typ, p.msg, p.done = status, handle, typ, msg, true
}

// Device errors: wgpu-native's own default handlers panic, which aborts the process when
// it happens across the C boundary. Hover's record the message instead (Device.Errors).
var deviceErrors struct {
	sync.Mutex
	m map[uintptr][]string
}

func deviceError(id uintptr, msg string) {
	deviceErrors.Lock()
	if deviceErrors.m == nil {
		deviceErrors.m = map[uintptr][]string{}
	}
	if len(deviceErrors.m[id]) < 64 {
		deviceErrors.m[id] = append(deviceErrors.m[id], msg)
	}
	deviceErrors.Unlock()
}

// ---- handles ---------------------------------------------------------------------------

type (
	Instance        struct{ h uintptr }
	Adapter         struct{ h uintptr }
	ShaderModule    struct{ h uintptr }
	BindGroupLayout struct{ h uintptr }
	BindGroup       struct{ h uintptr }
	PipelineLayout  struct{ h uintptr }
	RenderPipeline  struct{ h uintptr }
	Sampler         struct{ h uintptr }
	Buffer          struct{ h uintptr }
	Texture         struct{ h uintptr }
	TextureView     struct{ h uintptr }
	CommandEncoder  struct{ h uintptr }
	CommandBuffer   struct{ h uintptr }
	RenderPass      struct{ h uintptr }
	Queue           struct{ h uintptr }
)

// Device is a device, its queue and the id its error callbacks report under.
type Device struct {
	h     uintptr
	Queue Queue
	id    uintptr
}

func (x Instance) Release()        { call(fnInstanceRelease, x.h) }
func (x Adapter) Release()         { call(fnAdapterRelease, x.h) }
func (x ShaderModule) Release()    { call(fnShaderModuleRelease, x.h) }
func (x BindGroupLayout) Release() { call(fnBindGroupLayoutRelease, x.h) }
func (x BindGroup) Release()       { call(fnBindGroupRelease, x.h) }
func (x PipelineLayout) Release()  { call(fnPipelineLayoutRelease, x.h) }
func (x RenderPipeline) Release()  { call(fnRenderPipelineRelease, x.h) }
func (x Sampler) Release()         { call(fnSamplerRelease, x.h) }
func (x Buffer) Release()          { call(fnBufferRelease, x.h) }
func (x Texture) Release()         { call(fnTextureRelease, x.h) }
func (x TextureView) Release()     { call(fnTextureViewRelease, x.h) }
func (x CommandEncoder) Release()  { call(fnCommandEncoderRelease, x.h) }
func (x CommandBuffer) Release()   { call(fnCommandBufferRelease, x.h) }

func (d *Device) Release() {
	call(fnQueueRelease, d.Queue.h)
	call(fnDeviceRelease, d.h)
	deviceErrors.Lock()
	delete(deviceErrors.m, d.id)
	deviceErrors.Unlock()
}

// ---- instance, adapter, device --------------------------------------------------------

// CreateInstance makes an instance. On Windows it asks for DX12 only: a device on its
// own also woke Vulkan and OpenGL on every graphics card (the Rust office's note), and
// DX12 is what WARP, the runner's CPU adapter, speaks.
func CreateInstance() (Instance, error) {
	if err := Init(); err != nil {
		return Instance{}, err
	}
	var k pins
	defer k.done()
	desc := &wInstanceDescriptor{}
	if backends := instanceBackends(); backends != 0 {
		extras := &wInstanceExtras{chain: chainedStruct{sType: sTypeInstanceExtras}, backends: backends, dxcPath: noString}
		desc.nextInChain = ptr(&k, extras)
	}
	h := call(fnCreateInstance, ptr(&k, desc))
	if h == 0 {
		return Instance{}, errors.New("gpu: wgpuCreateInstance gave no instance")
	}
	return Instance{h}, nil
}

// RequestAdapter asks for a low-power adapter; fallback asks for the CPU one (WARP on
// Windows, llvmpipe on Linux).
func (inst Instance) RequestAdapter(fallback bool) (Adapter, error) {
	var k pins
	defer k.done()
	opts := &wRequestAdapterOptions{featureLevel: featureLevelCore, powerPreference: powerLowPower}
	if fallback {
		opts.forceFallbackAdapter = 1
	}
	id, p := newRequest()
	defer takeRequest(id)
	info := &wCallbackInfo{mode: callbackAllowProcess, callback: cbAdapter, userdata1: id}
	callWithInfo(fnInstanceRequestAdapter, []uintptr{inst.h, ptr(&k, opts)}, ptr(&k, info))
	// wgpu-native answers inside the call; ProcessEvents covers one that doesn't.
	for i := 0; i < 100 && !p.done; i++ {
		call(fnInstanceProcessEvents, inst.h)
	}
	switch {
	case !p.done:
		return Adapter{}, errors.New("gpu: no answer to the adapter request")
	case p.status != statusSuccess || p.handle == 0:
		return Adapter{}, fmt.Errorf("gpu: no adapter (status %d): %s", p.status, p.msg)
	}
	return Adapter{p.handle}, nil
}

// AdapterInfo is what the office logs and what decides software rendering.
type AdapterInfo struct {
	Name    string
	Backend BackendType
	Type    AdapterType
}

func (i AdapterInfo) String() string {
	return fmt.Sprintf("%s (backend %d, type %d)", i.Name, i.Backend, i.Type)
}

// ponytail: the info's strings are wgpu-native's and are not freed (a few bytes, once
// per adapter): wgpuAdapterInfoFreeMembers takes the 96-byte struct by value.
func (a Adapter) Info() AdapterInfo {
	var k pins
	defer k.done()
	info := &wAdapterInfo{}
	call(fnAdapterGetInfo, a.h, ptr(&k, info))
	return AdapterInfo{Name: goString(info.device), Backend: BackendType(info.backendType), Type: AdapterType(info.adapterType)}
}

var nextDevice struct {
	sync.Mutex
	n uintptr
}

func (inst Instance) RequestDevice(a Adapter, label string) (*Device, error) {
	var k pins
	defer k.done()
	nextDevice.Lock()
	nextDevice.n++
	devID := nextDevice.n
	nextDevice.Unlock()
	desc := &wDeviceDescriptor{
		label:        k.str(label),
		defaultQueue: wQueueDescriptor{label: noString},
		deviceLostCallbackInfo: wCallbackInfo{mode: callbackAllowSpontaneous, callback: cbDeviceLost,
			userdata1: devID},
		uncapturedErrorCallbackInfo: wUncapturedErrorCallbackInfo{callback: cbUncaptured, userdata1: devID},
	}
	id, p := newRequest()
	defer takeRequest(id)
	info := &wCallbackInfo{mode: callbackAllowProcess, callback: cbDevice, userdata1: id}
	callWithInfo(fnAdapterRequestDevice, []uintptr{a.h, ptr(&k, desc)}, ptr(&k, info))
	for i := 0; i < 100 && !p.done; i++ {
		call(fnInstanceProcessEvents, inst.h)
	}
	switch {
	case !p.done:
		return nil, errors.New("gpu: no answer to the device request")
	case p.status != statusSuccess || p.handle == 0:
		return nil, fmt.Errorf("gpu: no device (status %d): %s", p.status, p.msg)
	}
	return &Device{h: p.handle, Queue: Queue{call(fnDeviceGetQueue, p.handle)}, id: devID}, nil
}

// Errors returns and forgets the errors the device reported outside an error scope (a
// validation error, a lost device), oldest first.
func (d *Device) Errors() []string {
	deviceErrors.Lock()
	defer deviceErrors.Unlock()
	e := deviceErrors.m[d.id]
	delete(deviceErrors.m, d.id)
	return e
}

// Poll waits for the queue's work (wait) or only collects what has finished; map
// callbacks fire in it.
func (d *Device) Poll(wait bool) {
	w := uintptr(0)
	if wait {
		w = 1
	}
	call(fnDevicePoll, d.h, w, 0)
}

// ---- shaders and error scopes ---------------------------------------------------------

func (d *Device) ShaderModule(label, wgsl string) ShaderModule {
	var k pins
	defer k.done()
	src := &wShaderSourceWGSL{chain: chainedStruct{sType: sTypeShaderSourceWGSL}, code: k.str(wgsl)}
	desc := &wShaderModuleDescriptor{nextInChain: ptr(&k, src), label: k.str(label)}
	return ShaderModule{call(fnDeviceCreateShaderModule, d.h, ptr(&k, desc))}
}

// CheckShader compiles WGSL inside a validation error scope and returns wgpu's message,
// or "" when it compiled.
func (d *Device) CheckShader(inst Instance, label, wgsl string) string {
	call(fnDevicePushErrorScope, d.h, errorFilterValidation)
	m := d.ShaderModule(label, wgsl)
	id, p := newRequest()
	defer takeRequest(id)
	var k pins
	defer k.done()
	info := &wCallbackInfo{mode: callbackAllowProcess, callback: cbPopErrorScope, userdata1: id}
	callWithInfo(fnDevicePopErrorScope, []uintptr{d.h}, ptr(&k, info))
	for i := 0; i < 100 && !p.done; i++ {
		call(fnInstanceProcessEvents, inst.h)
	}
	if m.h != 0 {
		m.Release()
	}
	switch {
	case !p.done:
		return "no answer from the error scope"
	case p.status != statusSuccess:
		return fmt.Sprintf("the error scope failed (status %d): %s", p.status, p.msg)
	case p.typ != errorTypeNoError:
		if p.msg == "" {
			return fmt.Sprintf("error type %d", p.typ)
		}
		return p.msg
	}
	return ""
}

// ---- bind groups and pipelines -------------------------------------------------------

// LayoutEntry is one binding of a bind group layout: a uniform buffer, a filtering
// sampler or a 2D texture.
type LayoutEntry struct {
	Binding    uint32
	Visibility ShaderStage
	// Uniform: a uniform buffer; Dynamic: it takes a dynamic offset; MinSize: 0 for any.
	Uniform bool
	Dynamic bool
	MinSize uint64
	Sampler bool
	// Texture: its sample type (SampleFloat, SampleDepth), 0 for none.
	Texture SampleType
}

func (d *Device) BindGroupLayout(entries ...LayoutEntry) BindGroupLayout {
	var k pins
	defer k.done()
	w := make([]wBindGroupLayoutEntry, len(entries))
	for i, e := range entries {
		w[i] = wBindGroupLayoutEntry{binding: e.Binding, visibility: uint64(e.Visibility)}
		switch {
		case e.Uniform:
			w[i].buffer = wBufferBindingLayout{typ: bufferBindingUniform, minBindingSize: e.MinSize}
			if e.Dynamic {
				w[i].buffer.hasDynamicOffset = 1
			}
		case e.Sampler:
			w[i].sampler.typ = samplerBindingFilter
		case e.Texture != 0:
			w[i].texture = wTextureBindingLayout{sampleType: uint32(e.Texture), viewDimension: textureViewDimension2D}
		}
	}
	desc := &wBindGroupLayoutDescriptor{label: noString, entryCount: uintptr(len(w)), entries: slice(&k, w)}
	return BindGroupLayout{call(fnDeviceCreateBindGroupLayout, d.h, ptr(&k, desc))}
}

// GroupEntry is one resource of a bind group: a buffer range, a sampler or a view.
type GroupEntry struct {
	Binding uint32
	Buffer  Buffer
	Offset  uint64
	// Size 0 is the whole buffer.
	Size    uint64
	Sampler Sampler
	View    TextureView
}

func (d *Device) BindGroup(layout BindGroupLayout, entries ...GroupEntry) BindGroup {
	var k pins
	defer k.done()
	w := make([]wBindGroupEntry, len(entries))
	for i, e := range entries {
		w[i] = wBindGroupEntry{binding: e.Binding, buffer: e.Buffer.h, offset: e.Offset, size: e.Size, sampler: e.Sampler.h, textureView: e.View.h}
		if e.Buffer.h != 0 && e.Size == 0 {
			w[i].size = wholeSize
		}
	}
	desc := &wBindGroupDescriptor{label: noString, layout: layout.h, entryCount: uintptr(len(w)), entries: slice(&k, w)}
	return BindGroup{call(fnDeviceCreateBindGroup, d.h, ptr(&k, desc))}
}

func (d *Device) PipelineLayout(layouts ...BindGroupLayout) PipelineLayout {
	var k pins
	defer k.done()
	hs := make([]uintptr, len(layouts))
	for i, l := range layouts {
		hs[i] = l.h
	}
	desc := &wPipelineLayoutDescriptor{label: noString, bindGroupLayoutCount: uintptr(len(hs)), bindGroupLayouts: slice(&k, hs)}
	return PipelineLayout{call(fnDeviceCreatePipelineLayout, d.h, ptr(&k, desc))}
}

type VertexAttr struct {
	Format   VertexFormat
	Offset   uint64
	Location uint32
}

type VertexLayout struct {
	Stride uint64
	Attrs  []VertexAttr
}

type BlendComponent struct {
	Op       BlendOp
	Src, Dst BlendFactor
}

type Blend struct{ Color, Alpha BlendComponent }

type Target struct {
	Format TextureFormat
	// Blend nil writes the colour as it is.
	Blend *Blend
}

type DepthState struct {
	Format  TextureFormat
	Write   bool
	Compare Compare
}

// PipelineDesc describes a render pipeline. A zero Layout is the automatic one; an empty
// FS is no fragment stage (a depth-only pass).
type PipelineDesc struct {
	Label    string
	Layout   PipelineLayout
	Module   ShaderModule
	VS, FS   string
	Buffers  []VertexLayout
	Targets  []Target
	Topology Topology
	Front    FrontFace
	Cull     CullMode
	Depth    *DepthState
}

func (d *Device) RenderPipeline(p PipelineDesc) RenderPipeline {
	var k pins
	defer k.done()
	bufs := make([]wVertexBufferLayout, len(p.Buffers))
	for i, b := range p.Buffers {
		attrs := make([]wVertexAttribute, len(b.Attrs))
		for j, a := range b.Attrs {
			attrs[j] = wVertexAttribute{format: uint32(a.Format), offset: a.Offset, shaderLocation: a.Location}
		}
		bufs[i] = wVertexBufferLayout{stepMode: vertexStepModeVertex, arrayStride: b.Stride, attributeCount: uintptr(len(attrs)), attributes: slice(&k, attrs)}
	}
	topology := p.Topology
	if topology == 0 {
		topology = TriangleList
	}
	front := p.Front
	if front == 0 {
		front = FrontCCW
	}
	cull := p.Cull
	if cull == 0 {
		cull = CullNone
	}
	desc := &wRenderPipelineDescriptor{
		label:       k.str(p.Label),
		layout:      p.Layout.h,
		vertex:      wVertexState{module: p.Module.h, entryPoint: k.str(p.VS), bufferCount: uintptr(len(bufs)), buffers: slice(&k, bufs)},
		primitive:   wPrimitiveState{topology: uint32(topology), frontFace: uint32(front), cullMode: uint32(cull)},
		multisample: wMultisampleState{count: 1, mask: 0xFFFFFFFF},
	}
	if p.Depth != nil {
		keep := wStencilFaceState{compare: compareAlways, failOp: stencilKeep, depthFailOp: stencilKeep, passOp: stencilKeep}
		ds := &wDepthStencilState{format: uint32(p.Depth.Format), depthWriteEnabled: optionalBoolFalse, depthCompare: uint32(p.Depth.Compare),
			stencilFront: keep, stencilBack: keep, stencilReadMask: 0xFFFFFFFF, stencilWriteMask: 0xFFFFFFFF}
		if p.Depth.Write {
			ds.depthWriteEnabled = optionalBoolTrue
		}
		desc.depthStencil = ptr(&k, ds)
	}
	if p.FS != "" {
		ts := make([]wColorTargetState, len(p.Targets))
		for i, t := range p.Targets {
			ts[i] = wColorTargetState{format: uint32(t.Format), writeMask: colorWriteAll}
			if t.Blend != nil {
				b := &wBlendState{
					color: wBlendComponent{uint32(t.Blend.Color.Op), uint32(t.Blend.Color.Src), uint32(t.Blend.Color.Dst)},
					alpha: wBlendComponent{uint32(t.Blend.Alpha.Op), uint32(t.Blend.Alpha.Src), uint32(t.Blend.Alpha.Dst)},
				}
				ts[i].blend = ptr(&k, b)
			}
		}
		fs := &wFragmentState{module: p.Module.h, entryPoint: k.str(p.FS), targetCount: uintptr(len(ts)), targets: slice(&k, ts)}
		desc.fragment = ptr(&k, fs)
	}
	return RenderPipeline{call(fnDeviceCreateRenderPipeline, d.h, ptr(&k, desc))}
}

// ---- samplers, buffers, textures -----------------------------------------------------

type SamplerDesc struct {
	Mag, Min FilterMode
	AddressU AddressMode
	AddressV AddressMode
}

func (d *Device) Sampler(s SamplerDesc) Sampler {
	var k pins
	defer k.done()
	desc := &wSamplerDescriptor{label: noString, addressModeU: uint32(s.AddressU), addressModeV: uint32(s.AddressV), addressModeW: uint32(ClampToEdge),
		magFilter: uint32(s.Mag), minFilter: uint32(s.Min), mipmapFilter: uint32(FilterNearest), lodMinClamp: 0, lodMaxClamp: 32, maxAnisotropy: 1}
	return Sampler{call(fnDeviceCreateSampler, d.h, ptr(&k, desc))}
}

func (d *Device) Buffer(usage BufferUsage, size uint64) Buffer {
	var k pins
	defer k.done()
	desc := &wBufferDescriptor{label: noString, usage: uint64(usage), size: size}
	return Buffer{call(fnDeviceCreateBuffer, d.h, ptr(&k, desc))}
}

// BufferWith makes a buffer holding data (COPY_DST is added to its usage). Its size is
// rounded up to 4 bytes, as a buffer write needs.
func (d *Device) BufferWith(usage BufferUsage, data []byte) Buffer {
	n := (uint64(len(data)) + 3) &^ 3
	if n == 0 {
		n = 4
	}
	b := d.Buffer(usage|BufferCopyDst, n)
	if len(data)%4 != 0 {
		data = append(append([]byte(nil), data...), make([]byte, 4-len(data)%4)...)
	}
	d.Queue.WriteBuffer(b, 0, data)
	return b
}

type TextureDesc struct {
	Label  string
	W, H   uint32
	Format TextureFormat
	Usage  TextureUsage
}

func (d *Device) Texture(t TextureDesc) Texture {
	var k pins
	defer k.done()
	desc := &wTextureDescriptor{label: k.str(t.Label), usage: uint64(t.Usage), dimension: textureDimension2D,
		size: wExtent3D{t.W, t.H, 1}, format: uint32(t.Format), mipLevelCount: 1, sampleCount: 1}
	return Texture{call(fnDeviceCreateTexture, d.h, ptr(&k, desc))}
}

// View is the texture's default view.
func (t Texture) View() TextureView { return TextureView{call(fnTextureCreateView, t.h, 0)} }

// ---- the queue -------------------------------------------------------------------------

func (q Queue) Submit(cmds ...CommandBuffer) {
	if len(cmds) == 0 {
		return
	}
	var k pins
	defer k.done()
	hs := make([]uintptr, len(cmds))
	for i, c := range cmds {
		hs[i] = c.h
	}
	call(fnQueueSubmit, q.h, uintptr(len(hs)), slice(&k, hs))
}

func (q Queue) WriteBuffer(b Buffer, offset uint64, data []byte) {
	if len(data) == 0 {
		return
	}
	var k pins
	defer k.done()
	call(fnQueueWriteBuffer, q.h, b.h, uintptr(offset), slice(&k, data), uintptr(len(data)))
}

// WriteTexture fills a whole w x h texture from tightly packed rows (bytesPerRow apart).
func (q Queue) WriteTexture(t Texture, data []byte, bytesPerRow, w, h uint32) {
	if len(data) == 0 {
		return
	}
	var k pins
	defer k.done()
	dst := &wTexelCopyTextureInfo{texture: t.h, aspect: aspectAll}
	layout := &wTexelCopyBufferLayout{bytesPerRow: bytesPerRow, rowsPerImage: h}
	size := &wExtent3D{w, h, 1}
	call(fnQueueWriteTexture, q.h, ptr(&k, dst), slice(&k, data), uintptr(len(data)), ptr(&k, layout), ptr(&k, size))
}

// ---- encoding --------------------------------------------------------------------------

func (d *Device) Encoder() CommandEncoder {
	var k pins
	defer k.done()
	desc := &wCommandEncoderDescriptor{label: noString}
	return CommandEncoder{call(fnDeviceCreateCommandEncoder, d.h, ptr(&k, desc))}
}

type Color struct{ R, G, B, A float64 }

// ColorAttachment is cleared to Clear and stored.
type ColorAttachment struct {
	View  TextureView
	Clear Color
}

// DepthAttachment is cleared to Clear and stored.
type DepthAttachment struct {
	View  TextureView
	Clear float32
}

// RenderPass begins a pass. Color may be nil: a depth-only pass (the shadow map).
func (e CommandEncoder) RenderPass(label string, color *ColorAttachment, depth *DepthAttachment) RenderPass {
	var k pins
	defer k.done()
	desc := &wRenderPassDescriptor{label: k.str(label)}
	if color != nil {
		c := &wRenderPassColorAttachment{view: color.View.h, depthSlice: depthSliceUndefined, loadOp: loadOpClear, storeOp: storeOpStore,
			clearValue: wColor{color.Clear.R, color.Clear.G, color.Clear.B, color.Clear.A}}
		desc.colorAttachmentCount, desc.colorAttachments = 1, ptr(&k, c)
	}
	if depth != nil {
		ds := &wRenderPassDepthStencilAttachment{view: depth.View.h, depthLoadOp: loadOpClear, depthStoreOp: storeOpStore, depthClearValue: depth.Clear}
		desc.depthStencilAttachment = ptr(&k, ds)
	}
	return RenderPass{call(fnCommandEncoderBeginRenderPass, e.h, ptr(&k, desc))}
}

// CopyTextureToBuffer copies a whole w x h texture into a buffer, rows bytesPerRow apart
// (a multiple of 256).
func (e CommandEncoder) CopyTextureToBuffer(t Texture, b Buffer, bytesPerRow, w, h uint32) {
	var k pins
	defer k.done()
	src := &wTexelCopyTextureInfo{texture: t.h, aspect: aspectAll}
	dst := &wTexelCopyBufferInfo{layout: wTexelCopyBufferLayout{bytesPerRow: bytesPerRow, rowsPerImage: h}, buffer: b.h}
	size := &wExtent3D{w, h, 1}
	call(fnCommandEncoderCopyTextureToBuffer, e.h, ptr(&k, src), ptr(&k, dst), ptr(&k, size))
}

// Finish ends the encoder (and releases it) and returns its commands.
func (e CommandEncoder) Finish() CommandBuffer {
	var k pins
	defer k.done()
	desc := &wCommandBufferDescriptor{label: noString}
	c := CommandBuffer{call(fnCommandEncoderFinish, e.h, ptr(&k, desc))}
	e.Release()
	return c
}

func (p RenderPass) SetPipeline(pl RenderPipeline) { call(fnRenderPassSetPipeline, p.h, pl.h) }

func (p RenderPass) SetBindGroup(index uint32, g BindGroup, offsets ...uint32) {
	var k pins
	defer k.done()
	call(fnRenderPassSetBindGroup, p.h, uintptr(index), g.h, uintptr(len(offsets)), slice(&k, offsets))
}

func (p RenderPass) SetVertexBuffer(slot uint32, b Buffer) {
	call(fnRenderPassSetVertexBuffer, p.h, uintptr(slot), b.h, 0, wholeSize)
}

func (p RenderPass) SetIndexBuffer(b Buffer, f IndexFormat) {
	call(fnRenderPassSetIndexBuffer, p.h, b.h, uintptr(f), 0, wholeSize)
}

func (p RenderPass) Draw(vertices, instances, firstVertex, firstInstance uint32) {
	call(fnRenderPassDraw, p.h, uintptr(vertices), uintptr(instances), uintptr(firstVertex), uintptr(firstInstance))
}

func (p RenderPass) DrawIndexed(indices, instances, firstIndex uint32, baseVertex int32, firstInstance uint32) {
	call(fnRenderPassDrawIndexed, p.h, uintptr(indices), uintptr(instances), uintptr(firstIndex), uintptr(uint32(baseVertex)), uintptr(firstInstance))
}

// End ends the pass and releases it.
func (p RenderPass) End() {
	call(fnRenderPassEnd, p.h)
	call(fnRenderPassRelease, p.h)
}

// ---- reading back ----------------------------------------------------------------------

// MapRead maps size bytes of a MAP_READ buffer and waits for it. The bytes are Mapped's
// until Unmap.
func (b Buffer) MapRead(d *Device, size uint64) error {
	id, p := newRequest()
	defer takeRequest(id)
	var k pins
	defer k.done()
	info := &wCallbackInfo{mode: callbackAllowProcess, callback: cbMap, userdata1: id}
	callWithInfo(fnBufferMapAsync, []uintptr{b.h, mapModeRead, 0, uintptr(size)}, ptr(&k, info))
	for i := 0; i < 100 && !p.done; i++ {
		d.Poll(true)
	}
	switch {
	case !p.done:
		return errors.New("gpu: the buffer never mapped")
	case p.status != statusSuccess:
		return fmt.Errorf("gpu: map failed (status %d): %s", p.status, p.msg)
	}
	return nil
}

// Mapped is the mapped bytes. They are wgpu-native's and are gone after Unmap.
func (b Buffer) Mapped(size uint64) []byte {
	a := call(fnBufferGetMappedRange, b.h, 0, uintptr(size))
	if a == 0 {
		return nil
	}
	return unsafe.Slice(cPtr[byte](a), size)
}

func (b Buffer) Unmap() { call(fnBufferUnmap, b.h) }
