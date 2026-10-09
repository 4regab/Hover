// Package gpu is Hover's own binding to wgpu-native v29, covering the ~50 calls the
// office renderer needs. Every struct is laid out to match webgpu.h on the same platform
// (x86-64 Linux, Windows, macOS), and a test checks them against the C compiler.
//
// On Windows the DLL is called through syscall (no C compiler). On Linux and macOS it is
// loaded through goffi, which is already in the build through Gio.
//
// ponytail: the binding covers only what the office draws. Adding a call is one function
// and one line in the test. If the office ever needs compute or surfaces, add them here.
package gpu

import (
	"errors"
	"fmt"
	"math"
	"sync"
	"unsafe"
)

// Handle is a wgpu opaque handle (a pointer on the native side).
type Handle = uintptr

// StringView is WGPUStringView: data + length (not null-terminated).
type StringView struct {
	Data   uintptr
	Length uintptr
}

func sv(s string) StringView {
	if len(s) == 0 {
		return StringView{0, math.MaxUint64}
	}
	b := []byte(s)
	return StringView{uintptr(unsafe.Pointer(&b[0])), uintptr(len(b))}
}

// nullSV is the WGPU_STRING_VIEW_INIT sentinel: data=NULL, length=SIZE_MAX.
func nullSV() StringView { return StringView{0, math.MaxUint64} }

// ---- enums (the values wgpu-native v29 expects, not the Go constants) ----------------

type (
	BackendType       uint32
	AdapterType       uint32
	PowerPreference   uint32
	FeatureLevel      uint32
	CallbackMode      uint32
	RequestStatus     uint32
	ErrorFilter       uint32
	ErrorType         uint32
	MapMode           uint32
	MapAsyncStatus    uint32
	TextureFormat     uint32
	TextureUsage      uint64
	TextureDimension  uint32
	TextureViewDim    uint32
	TextureAspect     uint32
	TextureSampleType uint32
	BufferUsage       uint64
	BufferBindType    uint32
	SamplerBindType   uint32
	ShaderStage       uint64
	LoadOp            uint32
	StoreOp           uint32
	IndexFormat       uint32
	VertexFormat      uint32
	VertexStepMode    uint32
	PrimTopology      uint32
	FrontFace         uint32
	CullMode          uint32
	CompareFunc       uint32
	StencilOp         uint32
	BlendOp           uint32
	BlendFactor       uint32
	FilterMode        uint32
	MipmapFilterMode  uint32
	AddressMode       uint32
	ColorWriteMask    uint64
	OptionalBool      uint32
	SType             uint32
	PopErrorStatus    uint32
)

const (
	FeatureLevelCore        FeatureLevel   = 1
	CallbackModeAllowEvents CallbackMode   = 1
	RequestStatusSuccess    RequestStatus  = 1
	ErrorFilterValidation   ErrorFilter    = 1
	ErrorTypeNoError        ErrorType      = 1
	MapModeRead             MapMode        = 1
	MapAsyncStatusSuccess   MapAsyncStatus = 1
	PopErrorStatusSuccess   PopErrorStatus = 1

	PowerPreferenceLowPower PowerPreference = 1
	PowerPreferenceHighPerf PowerPreference = 2

	FormatRGBA8Unorm     TextureFormat = 0x12
	FormatRGBA8UnormSRGB TextureFormat = 0x13
	FormatRGBA16Float    TextureFormat = 0x20
	FormatDepth32Float   TextureFormat = 0x28

	TextureUsageNone           TextureUsage = 0
	TextureUsageCopySrc        TextureUsage = 1
	TextureUsageCopyDst        TextureUsage = 2
	TextureUsageTextureBinding TextureUsage = 4
	TextureUsageRenderAttach   TextureUsage = 0x10

	Dim2D TextureDimension = 2

	ViewDim2D TextureViewDim = 3

	AspectAll TextureAspect = 1

	SampleTypeFloat TextureSampleType = 2
	SampleTypeDepth TextureSampleType = 5

	BufUsageCopySrc BufferUsage = 4
	BufUsageCopyDst BufferUsage = 8
	BufUsageUniform BufferUsage = 0x40
	BufUsageVertex  BufferUsage = 0x20
	BufUsageIndex   BufferUsage = 0x10
	BufUsageMapRead BufferUsage = 1

	BufBindUniform BufferBindType = 2

	SamplerBindFiltering SamplerBindType = 2

	StageVertex   ShaderStage = 1
	StageFragment ShaderStage = 2
	StageVertFrag ShaderStage = 3

	LoadClear LoadOp  = 2
	StoreSt   StoreOp = 2

	IndexU32 IndexFormat = 2

	VFmtFloat32x2 VertexFormat = 8
	VFmtFloat32x3 VertexFormat = 9

	StepVertex VertexStepMode = 1

	TopoTriangles PrimTopology = 1
	TopoPoints    PrimTopology = 4

	FrontCCW FrontFace = 1
	FrontCW  FrontFace = 2

	CullNone  CullMode = 1
	CullFront CullMode = 2
	CullBack  CullMode = 3

	CompareLessEq CompareFunc = 4

	StencilKeep StencilOp = 1

	BlendAdd               BlendOp     = 1
	FactorOne              BlendFactor = 1
	FactorSrcAlpha         BlendFactor = 5
	FactorOneMinusSrcAlpha BlendFactor = 6

	FilterNearest FilterMode = 1
	FilterLinear  FilterMode = 2

	MipmapNearest MipmapFilterMode = 1
	MipmapLinear  MipmapFilterMode = 2

	AddrClampToEdge AddressMode = 3

	ColorWriteAll ColorWriteMask = 0xF

	OptBoolFalse OptionalBool = 1
	OptBoolTrue  OptionalBool = 2

	STypeShaderSourceWGSL SType = 0x305

	DepthSliceUndef uint32 = 0xFFFFFFFF
)

// ---- wire structs (match webgpu.h on x86-64) ----------------------------------------

// The offsets and sizes of every struct are checked in gpu_test.go against the C compiler.

type WChainedStruct struct {
	Next  uintptr // *WChainedStruct
	SType SType
	_pad  [4]byte
}

type WExtent3D struct{ Width, Height, DepthOrLayers uint32 }
type WOrigin3D struct{ X, Y, Z uint32 }
type WColor struct{ R, G, B, A float64 }

type WInstanceDescriptor struct {
	NextInChain uintptr
	Features    [2]uintptr // 2 pointers (requiredFeatureCount, requiredFeatures) — both 0
	_pad        [8]byte
}

type WRequestAdapterOptions struct {
	NextInChain       uintptr
	FeatureLevel      FeatureLevel
	PowerPreference   PowerPreference
	ForceFallback     uint32 // WGPUBool
	_pad              [4]byte
	CompatibleSurface uintptr
}

type WCallbackInfo struct {
	NextInChain uintptr
	Mode        CallbackMode
	_pad        [4]byte
	Callback    uintptr
	Userdata1   uintptr
	Userdata2   uintptr
}

type WDeviceDescriptor struct {
	NextInChain uintptr
	Label       StringView
	// The rest (requiredFeatureCount..uncapturedErrorCallbackInfo) is zeroed.
	_rest [120]byte
}

// DeviceDescriptorInit returns a properly initialized WGPUDeviceDescriptor with the
// sentinel values wgpu-native expects (NULL data, SIZE_MAX length for each StringView).
func DeviceDescriptorInit() WDeviceDescriptor {
	var d WDeviceDescriptor
	d.Label = nullSV()
	// defaultQueue.label at offset 56 from the start of _rest (= offset 80 in the struct):
	// _rest starts at byte 24, so defaultQueue.label.Length is at _rest offset 80-24+8 = 64-24+8 = 48+8 = 56.
	// Actually: the label (data+length) is at bytes 8..24 of the struct. _rest starts at byte 24.
	// defaultQueue is at byte 48 (24 bytes into _rest). Its label.Length is at 48+8=56 of _rest.
	// In the init dump: bytes 64..72 are ff ff ff ff ff ff ff ff.
	// 64 - 24 = 40 bytes into _rest.
	for i := 40; i < 48; i++ {
		d._rest[i] = 0xff
	}
	return d
}

type WShaderSourceWGSL struct {
	Chain WChainedStruct
	Code  StringView
}

type WShaderModuleDescriptor struct {
	NextInChain uintptr
	Label       StringView
}

type WBufferBindingLayout struct {
	NextInChain    uintptr
	Type           BufferBindType
	HasDynamicOff  uint32 // WGPUBool
	MinBindingSize uint64
}

type WSamplerBindingLayout struct {
	NextInChain uintptr
	Type        SamplerBindType
	_pad        [4]byte
}

type WTextureBindingLayout struct {
	NextInChain  uintptr
	SampleType   TextureSampleType
	ViewDim      TextureViewDim
	Multisampled uint32 // WGPUBool
	_pad         [4]byte
}

type WStorageTextureBindingLayout struct {
	NextInChain uintptr
	Access      uint32
	Format      TextureFormat
	ViewDim     TextureViewDim
	_pad        [4]byte
}

type WBindGroupLayoutEntry struct {
	NextInChain      uintptr
	Binding          uint32
	_pad1            [4]byte
	Visibility       ShaderStage
	BindingArraySize uint32
	_pad2            [4]byte
	Buffer           WBufferBindingLayout
	Sampler          WSamplerBindingLayout
	Texture          WTextureBindingLayout
	StorageTexture   WStorageTextureBindingLayout
}

type WBindGroupLayoutDescriptor struct {
	NextInChain uintptr
	Label       StringView
	EntryCount  uintptr
	Entries     uintptr // *WBindGroupLayoutEntry
}

type WBindGroupEntry struct {
	NextInChain uintptr
	Binding     uint32
	_pad        [4]byte
	Buffer      Handle
	Offset      uint64
	Size        uint64
	Sampler     Handle
	TextureView Handle
}

type WBindGroupDescriptor struct {
	NextInChain uintptr
	Label       StringView
	Layout      Handle
	EntryCount  uintptr
	Entries     uintptr // *WBindGroupEntry
}

type WPipelineLayoutDescriptor struct {
	NextInChain   uintptr
	Label         StringView
	BGLCount      uintptr
	BGLayouts     uintptr // *Handle
	ImmediateSize uint32
	_pad          [4]byte
}

type WVertexAttribute struct {
	NextInChain    uintptr
	Format         VertexFormat
	_pad           [4]byte
	Offset         uint64
	ShaderLocation uint32
	_pad2          [4]byte
}

type WVertexBufferLayout struct {
	NextInChain uintptr
	StepMode    VertexStepMode
	_pad        [4]byte
	ArrayStride uint64
	AttrCount   uintptr
	Attributes  uintptr // *WVertexAttribute
}

type WVertexState struct {
	NextInChain   uintptr
	Module        Handle
	EntryPoint    StringView
	ConstantCount uintptr
	Constants     uintptr
	BufferCount   uintptr
	Buffers       uintptr // *WVertexBufferLayout
}

type WBlendComponent struct {
	Op  BlendOp
	Src BlendFactor
	Dst BlendFactor
}

type WBlendState struct {
	Color WBlendComponent
	Alpha WBlendComponent
}

type WColorTargetState struct {
	NextInChain uintptr
	Format      TextureFormat
	_pad        [4]byte
	Blend       uintptr // *WBlendState, nullable
	WriteMask   ColorWriteMask
}

type WFragmentState struct {
	NextInChain   uintptr
	Module        Handle
	EntryPoint    StringView
	ConstantCount uintptr
	Constants     uintptr
	TargetCount   uintptr
	Targets       uintptr // *WColorTargetState
}

type WPrimitiveState struct {
	NextInChain      uintptr
	Topology         PrimTopology
	StripIndexFormat IndexFormat
	FrontFace        FrontFace
	CullMode         CullMode
	UnclippedDepth   uint32 // WGPUBool
	_pad             [4]byte
}

type WStencilFaceState struct {
	Compare     CompareFunc
	FailOp      StencilOp
	DepthFailOp StencilOp
	PassOp      StencilOp
}

type WDepthStencilState struct {
	NextInChain         uintptr
	Format              TextureFormat
	DepthWriteEnabled   OptionalBool
	DepthCompare        CompareFunc
	StencilFront        WStencilFaceState
	StencilBack         WStencilFaceState
	StencilReadMask     uint32
	StencilWriteMask    uint32
	DepthBias           int32
	DepthBiasSlopeScale float32
	DepthBiasClamp      float32
}

type WMultisampleState struct {
	NextInChain uintptr
	Count       uint32
	Mask        uint32
	AlphaToCov  uint32 // WGPUBool
	_pad        [4]byte
}

type WRenderPipelineDescriptor struct {
	NextInChain  uintptr
	Label        StringView
	Layout       Handle
	Vertex       WVertexState
	Primitive    WPrimitiveState
	DepthStencil uintptr // *WDepthStencilState, nullable
	Multisample  WMultisampleState
	Fragment     uintptr // *WFragmentState, nullable
}

type WSamplerDescriptor struct {
	NextInChain  uintptr
	Label        StringView
	AddressU     AddressMode
	AddressV     AddressMode
	AddressW     AddressMode
	MagFilter    FilterMode
	MinFilter    FilterMode
	MipmapFilter MipmapFilterMode
	LodMin       float32
	LodMax       float32
	Compare      CompareFunc
	MaxAniso     uint16
	_pad2        [2]byte
}

type WBufferDescriptor struct {
	NextInChain      uintptr
	Label            StringView
	Usage            BufferUsage
	Size             uint64
	MappedAtCreation uint32 // WGPUBool
	_pad             [4]byte
}

type WTextureDescriptor struct {
	NextInChain     uintptr
	Label           StringView
	Usage           TextureUsage
	Dimension       TextureDimension
	Size            WExtent3D
	Format          TextureFormat
	MipLevelCount   uint32
	SampleCount     uint32
	_pad            [4]byte
	ViewFormatCount uintptr
	ViewFormats     uintptr // *TextureFormat
}

type WTextureViewDescriptor struct {
	NextInChain     uintptr
	Label           StringView
	Format          TextureFormat
	Dimension       TextureViewDim
	BaseMipLevel    uint32
	MipLevelCount   uint32
	BaseArrayLayer  uint32
	ArrayLayerCount uint32
	Aspect          TextureAspect
	_pad            [4]byte
	Usage           TextureUsage
}

type WCommandEncoderDescriptor struct {
	NextInChain uintptr
	Label       StringView
}

type WRenderPassColorAttachment struct {
	NextInChain   uintptr
	View          Handle
	DepthSlice    uint32
	_pad          [4]byte
	ResolveTarget Handle
	LoadOp        LoadOp
	StoreOp       StoreOp
	ClearValue    WColor
}

type WRenderPassDepthStencilAttachment struct {
	NextInChain       uintptr
	View              Handle
	DepthLoadOp       LoadOp
	DepthStoreOp      StoreOp
	DepthClearValue   float32
	DepthReadOnly     uint32 // WGPUBool
	StencilLoadOp     LoadOp
	StencilStoreOp    StoreOp
	StencilClearValue uint32
	StencilReadOnly   uint32 // WGPUBool
}

type WRenderPassDescriptor struct {
	NextInChain    uintptr
	Label          StringView
	ColorCount     uintptr
	Colors         uintptr // *WRenderPassColorAttachment
	DepthStencil   uintptr // *WRenderPassDepthStencilAttachment, nullable
	OcclusionQuery uintptr
	Timestamp      uintptr
}

type WTexelCopyTextureInfo struct {
	Texture  Handle
	MipLevel uint32
	Origin   WOrigin3D
	Aspect   TextureAspect
	_pad     [4]byte
}

type WTexelCopyBufferInfo struct {
	Layout WTexelCopyBufferLayout
	Buffer Handle
}

type WTexelCopyBufferLayout struct {
	Offset       uint64
	BytesPerRow  uint32
	RowsPerImage uint32
}

type WCommandBufferDescriptor struct {
	NextInChain uintptr
	Label       StringView
}

type WAdapterInfo struct {
	NextInChain  uintptr
	Vendor       StringView
	Architecture StringView
	Device       StringView
	Description  StringView
	BackendType  BackendType
	AdapterType  AdapterType
	VendorID     uint32
	DeviceID     uint32
	_pad         [8]byte
}

// ---- the library (platform-specific loading is in gpu_windows.go / gpu_unix.go) ------

var (
	lib     library
	libOnce sync.Once
	libErr  error
)

// Init loads wgpu_native. It is called once; later calls return the first result.
func Init() error {
	libOnce.Do(func() { lib, libErr = loadLib() })
	return libErr
}

// ---- high-level API ------------------------------------------------------------------

type Instance struct{ h Handle }
type Adapter struct{ h Handle }
type Device struct{ h Handle }
type Queue struct{ h Handle }
type ShaderModule struct{ h Handle }
type BindGroupLayout struct{ h Handle }
type BindGroup struct{ h Handle }
type PipelineLayout struct{ h Handle }
type RenderPipeline struct{ h Handle }
type Sampler struct{ h Handle }
type Buffer struct{ h Handle }
type Texture struct{ h Handle }
type TextureView struct{ h Handle }
type CommandEncoder struct{ h Handle }
type CommandBuffer struct{ h Handle }
type RenderPass struct{ h Handle }

func (x Instance) Release()        { call("wgpuInstanceRelease", x.h) }
func (x Adapter) Release()         { call("wgpuAdapterRelease", x.h) }
func (x Device) Release()          { call("wgpuDeviceRelease", x.h) }
func (x Queue) Release()           { call("wgpuQueueRelease", x.h) }
func (x ShaderModule) Release()    { call("wgpuShaderModuleRelease", x.h) }
func (x BindGroupLayout) Release() { call("wgpuBindGroupLayoutRelease", x.h) }
func (x BindGroup) Release()       { call("wgpuBindGroupRelease", x.h) }
func (x PipelineLayout) Release()  { call("wgpuPipelineLayoutRelease", x.h) }
func (x RenderPipeline) Release()  { call("wgpuRenderPipelineRelease", x.h) }
func (x Sampler) Release()         { call("wgpuSamplerRelease", x.h) }
func (x Buffer) Release()          { call("wgpuBufferRelease", x.h) }
func (x Texture) Release()         { call("wgpuTextureRelease", x.h) }
func (x TextureView) Release()     { call("wgpuTextureViewRelease", x.h) }
func (x CommandEncoder) Release()  { call("wgpuCommandEncoderRelease", x.h) }
func (x CommandBuffer) Release()   { call("wgpuCommandBufferRelease", x.h) }
func (x RenderPass) Release()      { call("wgpuRenderPassEncoderRelease", x.h) }

func CreateInstance() (Instance, error) {
	desc := WInstanceDescriptor{}
	h := call("wgpuCreateInstance", uintptr(unsafe.Pointer(&desc)))
	if h == 0 {
		return Instance{}, errors.New("gpu: wgpuCreateInstance returned null")
	}
	return Instance{h}, nil
}

func (inst Instance) ProcessEvents() { call("wgpuInstanceProcessEvents", inst.h) }

func (inst Instance) RequestAdapter(opts *WRequestAdapterOptions) (Adapter, error) {
	return requestAdapter(inst, opts)
}

func (a Adapter) Info() WAdapterInfo {
	var info WAdapterInfo
	call("wgpuAdapterGetInfo", a.h, uintptr(unsafe.Pointer(&info)))
	return info
}

func (a Adapter) RequestDevice(inst Instance, desc *WDeviceDescriptor) (Device, Queue, error) {
	return requestDevice(inst, a, desc)
}

func (d Device) CreateShaderModuleWGSL(label, code string) ShaderModule {
	src := WShaderSourceWGSL{Chain: WChainedStruct{SType: STypeShaderSourceWGSL}, Code: sv(code)}
	desc := WShaderModuleDescriptor{NextInChain: uintptr(unsafe.Pointer(&src)), Label: sv(label)}
	return ShaderModule{call("wgpuDeviceCreateShaderModule", d.h, uintptr(unsafe.Pointer(&desc)))}
}

func (d Device) CreateBindGroupLayout(desc *WBindGroupLayoutDescriptor) BindGroupLayout {
	return BindGroupLayout{call("wgpuDeviceCreateBindGroupLayout", d.h, uintptr(unsafe.Pointer(desc)))}
}

func (d Device) CreateBindGroup(desc *WBindGroupDescriptor) BindGroup {
	return BindGroup{call("wgpuDeviceCreateBindGroup", d.h, uintptr(unsafe.Pointer(desc)))}
}

func (d Device) CreatePipelineLayout(desc *WPipelineLayoutDescriptor) PipelineLayout {
	return PipelineLayout{call("wgpuDeviceCreatePipelineLayout", d.h, uintptr(unsafe.Pointer(desc)))}
}

func (d Device) CreateRenderPipeline(desc *WRenderPipelineDescriptor) RenderPipeline {
	return RenderPipeline{call("wgpuDeviceCreateRenderPipeline", d.h, uintptr(unsafe.Pointer(desc)))}
}

func (d Device) CreateSampler(desc *WSamplerDescriptor) Sampler {
	return Sampler{call("wgpuDeviceCreateSampler", d.h, uintptr(unsafe.Pointer(desc)))}
}

func (d Device) CreateBuffer(desc *WBufferDescriptor) Buffer {
	return Buffer{call("wgpuDeviceCreateBuffer", d.h, uintptr(unsafe.Pointer(desc)))}
}

func (d Device) CreateTexture(desc *WTextureDescriptor) Texture {
	return Texture{call("wgpuDeviceCreateTexture", d.h, uintptr(unsafe.Pointer(desc)))}
}

func (d Device) CreateCommandEncoder(label string) CommandEncoder {
	desc := WCommandEncoderDescriptor{Label: sv(label)}
	return CommandEncoder{call("wgpuDeviceCreateCommandEncoder", d.h, uintptr(unsafe.Pointer(&desc)))}
}

func (d Device) Poll(wait bool) {
	w := uintptr(0)
	if wait {
		w = 1
	}
	call("wgpuDevicePoll", d.h, w, 0)
}

func (d Device) PushErrorScope(filter ErrorFilter) {
	call("wgpuDevicePushErrorScope", d.h, uintptr(filter))
}

func (d Device) PopErrorScope(inst Instance) (ErrorType, string) {
	return popErrorScope(d, inst)
}

func (t Texture) CreateView(desc *WTextureViewDescriptor) TextureView {
	var p uintptr
	if desc != nil {
		p = uintptr(unsafe.Pointer(desc))
	}
	return TextureView{call("wgpuTextureCreateView", t.h, p)}
}

func (q Queue) Submit(cmds ...CommandBuffer) {
	if len(cmds) == 0 {
		return
	}
	handles := make([]Handle, len(cmds))
	for i, c := range cmds {
		handles[i] = c.h
	}
	call("wgpuQueueSubmit", q.h, uintptr(len(handles)), uintptr(unsafe.Pointer(&handles[0])))
}

func (q Queue) WriteBuffer(buf Buffer, offset uint64, data []byte) {
	if len(data) == 0 {
		return
	}
	call("wgpuQueueWriteBuffer", q.h, buf.h, uintptr(offset), uintptr(unsafe.Pointer(&data[0])), uintptr(len(data)))
}

func (q Queue) WriteTexture(dst *WTexelCopyTextureInfo, data []byte, layout *WTexelCopyBufferLayout, size *WExtent3D) {
	if len(data) == 0 {
		return
	}
	call("wgpuQueueWriteTexture", q.h, uintptr(unsafe.Pointer(dst)), uintptr(unsafe.Pointer(&data[0])), uintptr(len(data)), uintptr(unsafe.Pointer(layout)), uintptr(unsafe.Pointer(size)))
}

func (enc CommandEncoder) BeginRenderPass(desc *WRenderPassDescriptor) RenderPass {
	return RenderPass{call("wgpuCommandEncoderBeginRenderPass", enc.h, uintptr(unsafe.Pointer(desc)))}
}

func (enc CommandEncoder) CopyTextureToBuffer(src *WTexelCopyTextureInfo, dst *WTexelCopyBufferInfo, size *WExtent3D) {
	call("wgpuCommandEncoderCopyTextureToBuffer", enc.h, uintptr(unsafe.Pointer(src)), uintptr(unsafe.Pointer(dst)), uintptr(unsafe.Pointer(size)))
}

func (enc CommandEncoder) Finish() CommandBuffer {
	desc := WCommandBufferDescriptor{Label: nullSV()}
	return CommandBuffer{call("wgpuCommandEncoderFinish", enc.h, uintptr(unsafe.Pointer(&desc)))}
}

func (p RenderPass) SetPipeline(pipe RenderPipeline) {
	call("wgpuRenderPassEncoderSetPipeline", p.h, pipe.h)
}

func (p RenderPass) SetBindGroup(slot uint32, bg BindGroup, offsets []uint32) {
	var op uintptr
	if len(offsets) > 0 {
		op = uintptr(unsafe.Pointer(&offsets[0]))
	}
	call("wgpuRenderPassEncoderSetBindGroup", p.h, uintptr(slot), bg.h, uintptr(len(offsets)), op)
}

func (p RenderPass) SetVertexBuffer(slot uint32, buf Buffer) {
	call("wgpuRenderPassEncoderSetVertexBuffer", p.h, uintptr(slot), buf.h, 0, 0xFFFFFFFFFFFFFFFF)
}

func (p RenderPass) SetIndexBuffer(buf Buffer, format IndexFormat) {
	call("wgpuRenderPassEncoderSetIndexBuffer", p.h, buf.h, uintptr(format), 0, 0xFFFFFFFFFFFFFFFF)
}

func (p RenderPass) Draw(vertexCount, instanceCount, firstVertex, firstInstance uint32) {
	call("wgpuRenderPassEncoderDraw", p.h, uintptr(vertexCount), uintptr(instanceCount), uintptr(firstVertex), uintptr(firstInstance))
}

func (p RenderPass) DrawIndexed(indexCount, instanceCount, firstIndex uint32, baseVertex int32, firstInstance uint32) {
	call("wgpuRenderPassEncoderDrawIndexed", p.h, uintptr(indexCount), uintptr(instanceCount), uintptr(firstIndex), uintptr(baseVertex), uintptr(firstInstance))
}

func (p RenderPass) End() {
	call("wgpuRenderPassEncoderEnd", p.h)
}

func (b Buffer) MapAsync(dev Device, mode MapMode, offset, size uint64) error {
	return bufferMapAsync(b, dev, mode, offset, size)
}

func (b Buffer) GetMappedRange(offset, size uint64) unsafe.Pointer {
	//nolint:govet // the handle is from wgpu
	return unsafe.Pointer(call("wgpuBufferGetMappedRange", b.h, uintptr(offset), uintptr(size)))
}

func (b Buffer) Unmap() {
	call("wgpuBufferUnmap", b.h)
}

// InfoString copies a StringView from wgpu-native memory into a Go string.
func InfoString(sv StringView) string {
	if sv.Data == 0 || sv.Length == 0 || sv.Length == math.MaxUint64 {
		return ""
	}
	//nolint:govet // copying from wgpu memory
	return string(unsafe.Slice((*byte)(unsafe.Pointer(sv.Data)), sv.Length))
}

// Fmt is the adapter info as a readable string.
func (info WAdapterInfo) Fmt() string {
	return fmt.Sprintf("%s (backend %d, type %d)", InfoString(info.Device), info.BackendType, info.AdapterType)
}

//go:nosplit
func noescape(p uintptr) unsafe.Pointer {
	x := p
	return unsafe.Pointer(x) //nolint:govet
}
