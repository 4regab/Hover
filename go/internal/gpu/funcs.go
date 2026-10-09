package gpu

// The wgpu-native functions the binding calls, resolved once when the library loads (a
// missing one fails Init, not a draw).
const (
	fnCreateInstance = iota
	fnInstanceRequestAdapter
	fnInstanceProcessEvents
	fnInstanceRelease
	fnAdapterGetInfo
	fnAdapterRequestDevice
	fnAdapterRelease
	fnDeviceGetQueue
	fnDevicePoll
	fnDevicePushErrorScope
	fnDevicePopErrorScope
	fnDeviceCreateShaderModule
	fnDeviceCreateBindGroupLayout
	fnDeviceCreateBindGroup
	fnDeviceCreatePipelineLayout
	fnDeviceCreateRenderPipeline
	fnDeviceCreateSampler
	fnDeviceCreateBuffer
	fnDeviceCreateTexture
	fnDeviceCreateCommandEncoder
	fnDeviceRelease
	fnQueueSubmit
	fnQueueWriteBuffer
	fnQueueWriteTexture
	fnQueueRelease
	fnTextureCreateView
	fnTextureRelease
	fnTextureViewRelease
	fnCommandEncoderBeginRenderPass
	fnCommandEncoderCopyTextureToBuffer
	fnCommandEncoderFinish
	fnCommandEncoderRelease
	fnCommandBufferRelease
	fnRenderPassSetPipeline
	fnRenderPassSetBindGroup
	fnRenderPassSetVertexBuffer
	fnRenderPassSetIndexBuffer
	fnRenderPassDraw
	fnRenderPassDrawIndexed
	fnRenderPassEnd
	fnRenderPassRelease
	fnBufferMapAsync
	fnBufferGetMappedRange
	fnBufferUnmap
	fnBufferRelease
	fnShaderModuleRelease
	fnBindGroupLayoutRelease
	fnBindGroupRelease
	fnPipelineLayoutRelease
	fnRenderPipelineRelease
	fnSamplerRelease
	fnCount
)

var fnNames = [fnCount]string{
	"wgpuCreateInstance",
	"wgpuInstanceRequestAdapter",
	"wgpuInstanceProcessEvents",
	"wgpuInstanceRelease",
	"wgpuAdapterGetInfo",
	"wgpuAdapterRequestDevice",
	"wgpuAdapterRelease",
	"wgpuDeviceGetQueue",
	"wgpuDevicePoll",
	"wgpuDevicePushErrorScope",
	"wgpuDevicePopErrorScope",
	"wgpuDeviceCreateShaderModule",
	"wgpuDeviceCreateBindGroupLayout",
	"wgpuDeviceCreateBindGroup",
	"wgpuDeviceCreatePipelineLayout",
	"wgpuDeviceCreateRenderPipeline",
	"wgpuDeviceCreateSampler",
	"wgpuDeviceCreateBuffer",
	"wgpuDeviceCreateTexture",
	"wgpuDeviceCreateCommandEncoder",
	"wgpuDeviceRelease",
	"wgpuQueueSubmit",
	"wgpuQueueWriteBuffer",
	"wgpuQueueWriteTexture",
	"wgpuQueueRelease",
	"wgpuTextureCreateView",
	"wgpuTextureRelease",
	"wgpuTextureViewRelease",
	"wgpuCommandEncoderBeginRenderPass",
	"wgpuCommandEncoderCopyTextureToBuffer",
	"wgpuCommandEncoderFinish",
	"wgpuCommandEncoderRelease",
	"wgpuCommandBufferRelease",
	"wgpuRenderPassEncoderSetPipeline",
	"wgpuRenderPassEncoderSetBindGroup",
	"wgpuRenderPassEncoderSetVertexBuffer",
	"wgpuRenderPassEncoderSetIndexBuffer",
	"wgpuRenderPassEncoderDraw",
	"wgpuRenderPassEncoderDrawIndexed",
	"wgpuRenderPassEncoderEnd",
	"wgpuRenderPassEncoderRelease",
	"wgpuBufferMapAsync",
	"wgpuBufferGetMappedRange",
	"wgpuBufferUnmap",
	"wgpuBufferRelease",
	"wgpuShaderModuleRelease",
	"wgpuBindGroupLayoutRelease",
	"wgpuBindGroupRelease",
	"wgpuPipelineLayoutRelease",
	"wgpuRenderPipelineRelease",
	"wgpuSamplerRelease",
}

// The callbacks, made once when the library loads (Windows and purego both keep every
// callback they make for the life of the process, so one per kind, routed by userdata).
var cbAdapter, cbDevice, cbMap, cbPopErrorScope, cbUncaptured, cbDeviceLost uintptr

// What the callbacks do, once the platform file has read their arguments.
func onAdapter(status, adapter uintptr, msg string, id uintptr) {
	complete(id, uint32(status), adapter, 0, msg)
}

func onDevice(status, device uintptr, msg string, id uintptr) {
	complete(id, uint32(status), device, 0, msg)
}

func onMap(status uintptr, msg string, id uintptr) { complete(id, uint32(status), 0, 0, msg) }

func onPopErrorScope(status, typ uintptr, msg string, id uintptr) {
	complete(id, uint32(status), 0, uint32(typ), msg)
}

func onUncaptured(typ uintptr, msg string, devID uintptr) {
	deviceError(devID, "uncaptured error (type "+itoa(typ)+"): "+msg)
}

func onDeviceLost(reason uintptr, msg string, devID uintptr) {
	deviceError(devID, "device lost (reason "+itoa(reason)+"): "+msg)
}

func itoa(v uintptr) string {
	if v == 0 {
		return "0"
	}
	var b [20]byte
	i := len(b)
	for ; v > 0; v /= 10 {
		i--
		b[i] = byte('0' + v%10)
	}
	return string(b[i:])
}
