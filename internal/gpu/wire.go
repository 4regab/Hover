package gpu

// The C structs of webgpu.h and wgpu.h (wgpu-native v29.0.0.0), field for field, for
// 64-bit Windows, Linux and macOS (pointers and size_t are 8 bytes on all three, enums
// 4). wire_test.go checks every size and offset against numbers the C compiler gave
// for the same header. Padding is spelled out so the Go layout can't drift.

import "math"

type stringView struct {
	data   uintptr
	length uintptr
}

// strlen is WGPU_STRLEN: a NULL view with this length is "no string" (the INIT value).
const strlen = math.MaxUint64

var noString = stringView{0, strlen}

type chainedStruct struct {
	next  uintptr
	sType uint32
	_     [4]byte
}

type wExtent3D struct{ width, height, depthOrArrayLayers uint32 }

type wOrigin3D struct{ x, y, z uint32 }

type wColor struct{ r, g, b, a float64 }

type wInstanceDescriptor struct {
	nextInChain          uintptr
	requiredFeatureCount uintptr
	requiredFeatures     uintptr
	requiredLimits       uintptr
}

// wInstanceExtras is wgpu.h's WGPUInstanceExtras, chained to the instance descriptor to
// pick the backends.
type wInstanceExtras struct {
	chain                   chainedStruct
	backends                uint64
	flags                   uint64
	dx12ShaderCompiler      uint32
	gles3MinorVersion       uint32
	glFenceBehaviour        uint32
	_                       [4]byte
	dxcPath                 stringView
	dxcMaxShaderModel       uint32
	dx12PresentationSystem  uint32
	budgetForDeviceCreation uintptr // const uint8_t * (nullable)
	budgetForDeviceLoss     uintptr
	_                       [24]byte // the rest of the struct, left at zero
}

type wRequestAdapterOptions struct {
	nextInChain          uintptr
	featureLevel         uint32
	powerPreference      uint32
	forceFallbackAdapter uint32
	backendType          uint32
	compatibleSurface    uintptr
}

// wCallbackInfo is the shape every WGPU*CallbackInfo with a mode has (request adapter,
// request device, buffer map, pop error scope, device lost).
type wCallbackInfo struct {
	nextInChain uintptr
	mode        uint32
	_           [4]byte
	callback    uintptr
	userdata1   uintptr
	userdata2   uintptr
}

type wUncapturedErrorCallbackInfo struct {
	nextInChain uintptr
	callback    uintptr
	userdata1   uintptr
	userdata2   uintptr
}

type wQueueDescriptor struct {
	nextInChain uintptr
	label       stringView
}

type wDeviceDescriptor struct {
	nextInChain                 uintptr
	label                       stringView
	requiredFeatureCount        uintptr
	requiredFeatures            uintptr
	requiredLimits              uintptr
	defaultQueue                wQueueDescriptor
	deviceLostCallbackInfo      wCallbackInfo
	uncapturedErrorCallbackInfo wUncapturedErrorCallbackInfo
}

type wShaderSourceWGSL struct {
	chain chainedStruct
	code  stringView
}

type wShaderModuleDescriptor struct {
	nextInChain uintptr
	label       stringView
}

type wBufferBindingLayout struct {
	nextInChain      uintptr
	typ              uint32
	hasDynamicOffset uint32
	minBindingSize   uint64
}

type wSamplerBindingLayout struct {
	nextInChain uintptr
	typ         uint32
	_           [4]byte
}

type wTextureBindingLayout struct {
	nextInChain   uintptr
	sampleType    uint32
	viewDimension uint32
	multisampled  uint32
	_             [4]byte
}

type wStorageTextureBindingLayout struct {
	nextInChain   uintptr
	access        uint32
	format        uint32
	viewDimension uint32
	_             [4]byte
}

type wBindGroupLayoutEntry struct {
	nextInChain      uintptr
	binding          uint32
	_                [4]byte
	visibility       uint64
	bindingArraySize uint32
	_                [4]byte
	buffer           wBufferBindingLayout
	sampler          wSamplerBindingLayout
	texture          wTextureBindingLayout
	storageTexture   wStorageTextureBindingLayout
}

type wBindGroupLayoutDescriptor struct {
	nextInChain uintptr
	label       stringView
	entryCount  uintptr
	entries     uintptr
}

type wBindGroupEntry struct {
	nextInChain uintptr
	binding     uint32
	_           [4]byte
	buffer      uintptr
	offset      uint64
	size        uint64
	sampler     uintptr
	textureView uintptr
}

type wBindGroupDescriptor struct {
	nextInChain uintptr
	label       stringView
	layout      uintptr
	entryCount  uintptr
	entries     uintptr
}

type wPipelineLayoutDescriptor struct {
	nextInChain          uintptr
	label                stringView
	bindGroupLayoutCount uintptr
	bindGroupLayouts     uintptr
	immediateSize        uint32
	_                    [4]byte
}

type wVertexAttribute struct {
	nextInChain    uintptr
	format         uint32
	_              [4]byte
	offset         uint64
	shaderLocation uint32
	_              [4]byte
}

type wVertexBufferLayout struct {
	nextInChain    uintptr
	stepMode       uint32
	_              [4]byte
	arrayStride    uint64
	attributeCount uintptr
	attributes     uintptr
}

type wVertexState struct {
	nextInChain   uintptr
	module        uintptr
	entryPoint    stringView
	constantCount uintptr
	constants     uintptr
	bufferCount   uintptr
	buffers       uintptr
}

type wBlendComponent struct{ operation, srcFactor, dstFactor uint32 }

type wBlendState struct{ color, alpha wBlendComponent }

type wColorTargetState struct {
	nextInChain uintptr
	format      uint32
	_           [4]byte
	blend       uintptr
	writeMask   uint64
}

type wFragmentState struct {
	nextInChain   uintptr
	module        uintptr
	entryPoint    stringView
	constantCount uintptr
	constants     uintptr
	targetCount   uintptr
	targets       uintptr
}

type wPrimitiveState struct {
	nextInChain      uintptr
	topology         uint32
	stripIndexFormat uint32
	frontFace        uint32
	cullMode         uint32
	unclippedDepth   uint32
	_                [4]byte
}

type wStencilFaceState struct{ compare, failOp, depthFailOp, passOp uint32 }

type wDepthStencilState struct {
	nextInChain         uintptr
	format              uint32
	depthWriteEnabled   uint32 // WGPUOptionalBool
	depthCompare        uint32
	stencilFront        wStencilFaceState
	stencilBack         wStencilFaceState
	stencilReadMask     uint32
	stencilWriteMask    uint32
	depthBias           int32
	depthBiasSlopeScale float32
	depthBiasClamp      float32
}

type wMultisampleState struct {
	nextInChain            uintptr
	count                  uint32
	mask                   uint32
	alphaToCoverageEnabled uint32
	_                      [4]byte
}

type wRenderPipelineDescriptor struct {
	nextInChain  uintptr
	label        stringView
	layout       uintptr
	vertex       wVertexState
	primitive    wPrimitiveState
	depthStencil uintptr
	multisample  wMultisampleState
	fragment     uintptr
}

type wSamplerDescriptor struct {
	nextInChain   uintptr
	label         stringView
	addressModeU  uint32
	addressModeV  uint32
	addressModeW  uint32
	magFilter     uint32
	minFilter     uint32
	mipmapFilter  uint32
	lodMinClamp   float32
	lodMaxClamp   float32
	compare       uint32
	maxAnisotropy uint16
	_             [2]byte
}

type wBufferDescriptor struct {
	nextInChain      uintptr
	label            stringView
	usage            uint64
	size             uint64
	mappedAtCreation uint32
	_                [4]byte
}

type wTextureDescriptor struct {
	nextInChain     uintptr
	label           stringView
	usage           uint64
	dimension       uint32
	size            wExtent3D
	format          uint32
	mipLevelCount   uint32
	sampleCount     uint32
	_               [4]byte
	viewFormatCount uintptr
	viewFormats     uintptr
}

type wCommandEncoderDescriptor struct {
	nextInChain uintptr
	label       stringView
}

type wCommandBufferDescriptor struct {
	nextInChain uintptr
	label       stringView
}

type wRenderPassColorAttachment struct {
	nextInChain   uintptr
	view          uintptr
	depthSlice    uint32
	_             [4]byte
	resolveTarget uintptr
	loadOp        uint32
	storeOp       uint32
	clearValue    wColor
}

type wRenderPassDepthStencilAttachment struct {
	nextInChain       uintptr
	view              uintptr
	depthLoadOp       uint32
	depthStoreOp      uint32
	depthClearValue   float32
	depthReadOnly     uint32
	stencilLoadOp     uint32
	stencilStoreOp    uint32
	stencilClearValue uint32
	stencilReadOnly   uint32
}

type wRenderPassDescriptor struct {
	nextInChain            uintptr
	label                  stringView
	colorAttachmentCount   uintptr
	colorAttachments       uintptr
	depthStencilAttachment uintptr
	occlusionQuerySet      uintptr
	timestampWrites        uintptr
}

type wTexelCopyTextureInfo struct {
	texture  uintptr
	mipLevel uint32
	origin   wOrigin3D
	aspect   uint32
	_        [4]byte
}

type wTexelCopyBufferLayout struct {
	offset       uint64
	bytesPerRow  uint32
	rowsPerImage uint32
}

type wTexelCopyBufferInfo struct {
	layout wTexelCopyBufferLayout
	buffer uintptr
}

type wAdapterInfo struct {
	nextInChain     uintptr
	vendor          stringView
	architecture    stringView
	device          stringView
	description     stringView
	backendType     uint32
	adapterType     uint32
	vendorID        uint32
	deviceID        uint32
	subgroupMinSize uint32
	subgroupMaxSize uint32
}

// ---- the enum values (webgpu.h v29's numbers, not the WebGPU IDL's order) ----------

const (
	sTypeShaderSourceWGSL = 0x2
	sTypeInstanceExtras   = 0x30006

	instanceBackendVulkan = 1 << 0
	instanceBackendGL     = 1 << 1
	instanceBackendDX12   = 1 << 3

	featureLevelCore         = 2
	callbackAllowProcess     = 2
	callbackAllowSpontaneous = 3
	statusSuccess            = 1 // request adapter/device, map, pop error scope

	powerLowPower = 1

	errorFilterValidation = 1
	errorTypeNoError      = 1

	mapModeRead = 1

	textureDimension2D = 2

	bufferBindingUniform   = 2
	samplerBindingFilter   = 2
	textureViewDimension2D = 2

	loadOpClear  = 2
	storeOpStore = 1

	vertexStepModeVertex = 1

	optionalBoolFalse = 0
	optionalBoolTrue  = 1

	stencilKeep   = 1
	compareAlways = 8

	colorWriteAll = 0xF

	depthSliceUndefined = 0xFFFFFFFF
	wholeSize           = math.MaxUint64

	aspectAll = 1
)
