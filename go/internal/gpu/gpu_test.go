package gpu

import (
	"testing"
	"unsafe"
)

// TestStructLayouts checks every Go wire struct against the sizes and offsets measured
// from webgpu.h with the C compiler (in /projects/sandbox/zzlayout/layout.txt, and
// reproduced here as constants so the test runs on every machine). The test is the only
// thing that keeps the binding correct when wgpu-native changes a struct.

func TestStructLayouts(t *testing.T) {
	type field struct {
		name   string
		offset uintptr
	}
	type check struct {
		name   string
		size   uintptr
		fields []field
	}
	checks := []check{
		{"WGPUStringView", 16, nil},
		{"WGPUChainedStruct", 16, []field{{"next", 0}, {"sType", 8}}},
		{"WGPUExtent3D", 12, nil},
		{"WGPUOrigin3D", 12, nil},
		{"WGPUColor", 32, nil},
		{"WGPURequestAdapterCallbackInfo", 40, []field{{"mode", 8}, {"callback", 16}, {"userdata1", 24}, {"userdata2", 32}}},
		{"WGPURequestDeviceCallbackInfo", 40, nil},
		{"WGPUBufferMapCallbackInfo", 40, nil},
		{"WGPUPopErrorScopeCallbackInfo", 40, nil},
		{"WGPUShaderSourceWGSL", 32, []field{{"chain", 0}, {"code", 16}}},
		{"WGPUShaderModuleDescriptor", 24, []field{{"nextInChain", 0}, {"label", 8}}},
		{"WGPUBindGroupLayoutEntry", 120, []field{{"binding", 8}, {"visibility", 16}, {"bindingArraySize", 24}, {"buffer", 32}, {"sampler", 56}, {"texture", 72}, {"storageTexture", 96}}},
		{"WGPUBindGroupLayoutDescriptor", 40, []field{{"entryCount", 24}, {"entries", 32}}},
		{"WGPUBindGroupEntry", 56, []field{{"binding", 8}, {"buffer", 16}, {"offset", 24}, {"size", 32}, {"sampler", 40}, {"textureView", 48}}},
		{"WGPUBindGroupDescriptor", 48, []field{{"layout", 24}, {"entryCount", 32}, {"entries", 40}}},
		{"WGPUPipelineLayoutDescriptor", 48, []field{{"bindGroupLayoutCount", 24}, {"bindGroupLayouts", 32}}},
		{"WGPUVertexAttribute", 32, []field{{"format", 8}, {"offset", 16}, {"shaderLocation", 24}}},
		{"WGPUVertexBufferLayout", 40, []field{{"stepMode", 8}, {"arrayStride", 16}, {"attributeCount", 24}, {"attributes", 32}}},
		{"WGPUVertexState", 64, []field{{"module", 8}, {"entryPoint", 16}, {"constantCount", 32}, {"constants", 40}, {"bufferCount", 48}, {"buffers", 56}}},
		{"WGPUBlendComponent", 12, nil},
		{"WGPUBlendState", 24, nil},
		{"WGPUColorTargetState", 32, []field{{"format", 8}, {"blend", 16}, {"writeMask", 24}}},
		{"WGPUFragmentState", 64, []field{{"module", 8}, {"entryPoint", 16}, {"constantCount", 32}, {"constants", 40}, {"targetCount", 48}, {"targets", 56}}},
		{"WGPUPrimitiveState", 32, []field{{"topology", 8}, {"stripIndexFormat", 12}, {"frontFace", 16}, {"cullMode", 20}, {"unclippedDepth", 24}}},
		{"WGPUStencilFaceState", 16, nil},
		{"WGPUDepthStencilState", 72, []field{{"format", 8}, {"depthWriteEnabled", 12}, {"depthCompare", 16}, {"stencilFront", 20}, {"stencilBack", 36}, {"stencilReadMask", 52}, {"stencilWriteMask", 56}, {"depthBias", 60}, {"depthBiasSlopeScale", 64}, {"depthBiasClamp", 68}}},
		{"WGPUMultisampleState", 24, []field{{"count", 8}, {"mask", 12}, {"alphaToCoverageEnabled", 16}}},
		{"WGPURenderPipelineDescriptor", 168, []field{{"layout", 24}, {"vertex", 32}, {"primitive", 96}, {"depthStencil", 128}, {"multisample", 136}, {"fragment", 160}}},
		{"WGPUSamplerDescriptor", 64, []field{{"addressModeU", 24}, {"addressModeV", 28}, {"magFilter", 36}, {"minFilter", 40}, {"mipmapFilter", 44}, {"lodMinClamp", 48}, {"lodMaxClamp", 52}, {"compare", 56}, {"maxAnisotropy", 60}}},
		{"WGPUBufferDescriptor", 48, []field{{"usage", 24}, {"size", 32}, {"mappedAtCreation", 40}}},
		{"WGPUTextureDescriptor", 80, []field{{"usage", 24}, {"dimension", 32}, {"size", 36}, {"format", 48}, {"mipLevelCount", 52}, {"sampleCount", 56}, {"viewFormatCount", 64}, {"viewFormats", 72}}},
		{"WGPUCommandEncoderDescriptor", 24, []field{{"label", 8}}},
		{"WGPURenderPassColorAttachment", 72, []field{{"view", 8}, {"depthSlice", 16}, {"resolveTarget", 24}, {"loadOp", 32}, {"storeOp", 36}, {"clearValue", 40}}},
		{"WGPURenderPassDepthStencilAttachment", 48, []field{{"view", 8}, {"depthLoadOp", 16}, {"depthStoreOp", 20}, {"depthClearValue", 24}, {"depthReadOnly", 28}, {"stencilLoadOp", 32}, {"stencilStoreOp", 36}, {"stencilClearValue", 40}, {"stencilReadOnly", 44}}},
		{"WGPURenderPassDescriptor", 64, []field{{"colorAttachmentCount", 24}, {"colorAttachments", 32}, {"depthStencilAttachment", 40}}},
		{"WGPUTexelCopyTextureInfo", 32, []field{{"texture", 0}, {"mipLevel", 8}, {"origin", 12}, {"aspect", 24}}},
		{"WGPUTexelCopyBufferInfo", 24, []field{{"layout", 0}, {"buffer", 16}}},
		{"WGPUTexelCopyBufferLayout", 16, nil},
		{"WGPUCommandBufferDescriptor", 24, nil},
		{"WGPUAdapterInfo", 96, []field{{"vendor", 8}, {"architecture", 24}, {"device", 40}, {"description", 56}, {"backendType", 72}, {"adapterType", 76}, {"vendorID", 80}, {"deviceID", 84}}},
		{"WGPUBufferBindingLayout", 24, []field{{"type", 8}, {"hasDynamicOffset", 12}, {"minBindingSize", 16}}},
		{"WGPUSamplerBindingLayout", 16, []field{{"type", 8}}},
		{"WGPUTextureBindingLayout", 24, []field{{"sampleType", 8}, {"viewDimension", 12}, {"multisampled", 16}}},
		{"WGPUStorageTextureBindingLayout", 24, []field{{"access", 8}, {"format", 12}, {"viewDimension", 16}}},
		{"WGPUTextureViewDescriptor", 64, []field{{"format", 24}, {"dimension", 28}, {"baseMipLevel", 32}, {"mipLevelCount", 36}, {"baseArrayLayer", 40}, {"arrayLayerCount", 44}, {"aspect", 48}, {"usage", 56}}},
	}
	// Now check the Go structs.
	type goStruct struct {
		name   string
		size   uintptr
		fields map[string]uintptr
	}
	goStructs := map[string]goStruct{
		"WGPUStringView":                       {size: unsafe.Sizeof(StringView{})},
		"WGPUChainedStruct":                    {size: unsafe.Sizeof(WChainedStruct{}), fields: map[string]uintptr{"next": unsafe.Offsetof(WChainedStruct{}.Next), "sType": unsafe.Offsetof(WChainedStruct{}.SType)}},
		"WGPUExtent3D":                         {size: unsafe.Sizeof(WExtent3D{})},
		"WGPUOrigin3D":                         {size: unsafe.Sizeof(WOrigin3D{})},
		"WGPUColor":                            {size: unsafe.Sizeof(WColor{})},
		"WGPURequestAdapterCallbackInfo":       {size: unsafe.Sizeof(WCallbackInfo{}), fields: map[string]uintptr{"mode": unsafe.Offsetof(WCallbackInfo{}.Mode), "callback": unsafe.Offsetof(WCallbackInfo{}.Callback), "userdata1": unsafe.Offsetof(WCallbackInfo{}.Userdata1), "userdata2": unsafe.Offsetof(WCallbackInfo{}.Userdata2)}},
		"WGPURequestDeviceCallbackInfo":        {size: unsafe.Sizeof(WCallbackInfo{})},
		"WGPUBufferMapCallbackInfo":            {size: unsafe.Sizeof(WCallbackInfo{})},
		"WGPUPopErrorScopeCallbackInfo":        {size: unsafe.Sizeof(WCallbackInfo{})},
		"WGPUShaderSourceWGSL":                 {size: unsafe.Sizeof(WShaderSourceWGSL{}), fields: map[string]uintptr{"chain": unsafe.Offsetof(WShaderSourceWGSL{}.Chain), "code": unsafe.Offsetof(WShaderSourceWGSL{}.Code)}},
		"WGPUShaderModuleDescriptor":           {size: unsafe.Sizeof(WShaderModuleDescriptor{}), fields: map[string]uintptr{"nextInChain": unsafe.Offsetof(WShaderModuleDescriptor{}.NextInChain), "label": unsafe.Offsetof(WShaderModuleDescriptor{}.Label)}},
		"WGPUBindGroupLayoutEntry":             {size: unsafe.Sizeof(WBindGroupLayoutEntry{}), fields: map[string]uintptr{"binding": unsafe.Offsetof(WBindGroupLayoutEntry{}.Binding), "visibility": unsafe.Offsetof(WBindGroupLayoutEntry{}.Visibility), "bindingArraySize": unsafe.Offsetof(WBindGroupLayoutEntry{}.BindingArraySize), "buffer": unsafe.Offsetof(WBindGroupLayoutEntry{}.Buffer), "sampler": unsafe.Offsetof(WBindGroupLayoutEntry{}.Sampler), "texture": unsafe.Offsetof(WBindGroupLayoutEntry{}.Texture), "storageTexture": unsafe.Offsetof(WBindGroupLayoutEntry{}.StorageTexture)}},
		"WGPUBindGroupLayoutDescriptor":        {size: unsafe.Sizeof(WBindGroupLayoutDescriptor{}), fields: map[string]uintptr{"entryCount": unsafe.Offsetof(WBindGroupLayoutDescriptor{}.EntryCount), "entries": unsafe.Offsetof(WBindGroupLayoutDescriptor{}.Entries)}},
		"WGPUBindGroupEntry":                   {size: unsafe.Sizeof(WBindGroupEntry{}), fields: map[string]uintptr{"binding": unsafe.Offsetof(WBindGroupEntry{}.Binding), "buffer": unsafe.Offsetof(WBindGroupEntry{}.Buffer), "offset": unsafe.Offsetof(WBindGroupEntry{}.Offset), "size": unsafe.Offsetof(WBindGroupEntry{}.Size), "sampler": unsafe.Offsetof(WBindGroupEntry{}.Sampler), "textureView": unsafe.Offsetof(WBindGroupEntry{}.TextureView)}},
		"WGPUBindGroupDescriptor":              {size: unsafe.Sizeof(WBindGroupDescriptor{}), fields: map[string]uintptr{"layout": unsafe.Offsetof(WBindGroupDescriptor{}.Layout), "entryCount": unsafe.Offsetof(WBindGroupDescriptor{}.EntryCount), "entries": unsafe.Offsetof(WBindGroupDescriptor{}.Entries)}},
		"WGPUPipelineLayoutDescriptor":         {size: unsafe.Sizeof(WPipelineLayoutDescriptor{}), fields: map[string]uintptr{"bindGroupLayoutCount": unsafe.Offsetof(WPipelineLayoutDescriptor{}.BGLCount), "bindGroupLayouts": unsafe.Offsetof(WPipelineLayoutDescriptor{}.BGLayouts)}},
		"WGPUVertexAttribute":                  {size: unsafe.Sizeof(WVertexAttribute{}), fields: map[string]uintptr{"format": unsafe.Offsetof(WVertexAttribute{}.Format), "offset": unsafe.Offsetof(WVertexAttribute{}.Offset), "shaderLocation": unsafe.Offsetof(WVertexAttribute{}.ShaderLocation)}},
		"WGPUVertexBufferLayout":               {size: unsafe.Sizeof(WVertexBufferLayout{}), fields: map[string]uintptr{"stepMode": unsafe.Offsetof(WVertexBufferLayout{}.StepMode), "arrayStride": unsafe.Offsetof(WVertexBufferLayout{}.ArrayStride), "attributeCount": unsafe.Offsetof(WVertexBufferLayout{}.AttrCount), "attributes": unsafe.Offsetof(WVertexBufferLayout{}.Attributes)}},
		"WGPUVertexState":                      {size: unsafe.Sizeof(WVertexState{}), fields: map[string]uintptr{"module": unsafe.Offsetof(WVertexState{}.Module), "entryPoint": unsafe.Offsetof(WVertexState{}.EntryPoint), "constantCount": unsafe.Offsetof(WVertexState{}.ConstantCount), "constants": unsafe.Offsetof(WVertexState{}.Constants), "bufferCount": unsafe.Offsetof(WVertexState{}.BufferCount), "buffers": unsafe.Offsetof(WVertexState{}.Buffers)}},
		"WGPUBlendComponent":                   {size: unsafe.Sizeof(WBlendComponent{})},
		"WGPUBlendState":                       {size: unsafe.Sizeof(WBlendState{})},
		"WGPUColorTargetState":                 {size: unsafe.Sizeof(WColorTargetState{}), fields: map[string]uintptr{"format": unsafe.Offsetof(WColorTargetState{}.Format), "blend": unsafe.Offsetof(WColorTargetState{}.Blend), "writeMask": unsafe.Offsetof(WColorTargetState{}.WriteMask)}},
		"WGPUFragmentState":                    {size: unsafe.Sizeof(WFragmentState{}), fields: map[string]uintptr{"module": unsafe.Offsetof(WFragmentState{}.Module), "entryPoint": unsafe.Offsetof(WFragmentState{}.EntryPoint), "constantCount": unsafe.Offsetof(WFragmentState{}.ConstantCount), "constants": unsafe.Offsetof(WFragmentState{}.Constants), "targetCount": unsafe.Offsetof(WFragmentState{}.TargetCount), "targets": unsafe.Offsetof(WFragmentState{}.Targets)}},
		"WGPUPrimitiveState":                   {size: unsafe.Sizeof(WPrimitiveState{}), fields: map[string]uintptr{"topology": unsafe.Offsetof(WPrimitiveState{}.Topology), "stripIndexFormat": unsafe.Offsetof(WPrimitiveState{}.StripIndexFormat), "frontFace": unsafe.Offsetof(WPrimitiveState{}.FrontFace), "cullMode": unsafe.Offsetof(WPrimitiveState{}.CullMode), "unclippedDepth": unsafe.Offsetof(WPrimitiveState{}.UnclippedDepth)}},
		"WGPUStencilFaceState":                 {size: unsafe.Sizeof(WStencilFaceState{})},
		"WGPUDepthStencilState":                {size: unsafe.Sizeof(WDepthStencilState{}), fields: map[string]uintptr{"format": unsafe.Offsetof(WDepthStencilState{}.Format), "depthWriteEnabled": unsafe.Offsetof(WDepthStencilState{}.DepthWriteEnabled), "depthCompare": unsafe.Offsetof(WDepthStencilState{}.DepthCompare), "stencilFront": unsafe.Offsetof(WDepthStencilState{}.StencilFront), "stencilBack": unsafe.Offsetof(WDepthStencilState{}.StencilBack), "stencilReadMask": unsafe.Offsetof(WDepthStencilState{}.StencilReadMask), "stencilWriteMask": unsafe.Offsetof(WDepthStencilState{}.StencilWriteMask), "depthBias": unsafe.Offsetof(WDepthStencilState{}.DepthBias), "depthBiasSlopeScale": unsafe.Offsetof(WDepthStencilState{}.DepthBiasSlopeScale), "depthBiasClamp": unsafe.Offsetof(WDepthStencilState{}.DepthBiasClamp)}},
		"WGPUMultisampleState":                 {size: unsafe.Sizeof(WMultisampleState{}), fields: map[string]uintptr{"count": unsafe.Offsetof(WMultisampleState{}.Count), "mask": unsafe.Offsetof(WMultisampleState{}.Mask), "alphaToCoverageEnabled": unsafe.Offsetof(WMultisampleState{}.AlphaToCov)}},
		"WGPURenderPipelineDescriptor":         {size: unsafe.Sizeof(WRenderPipelineDescriptor{}), fields: map[string]uintptr{"layout": unsafe.Offsetof(WRenderPipelineDescriptor{}.Layout), "vertex": unsafe.Offsetof(WRenderPipelineDescriptor{}.Vertex), "primitive": unsafe.Offsetof(WRenderPipelineDescriptor{}.Primitive), "depthStencil": unsafe.Offsetof(WRenderPipelineDescriptor{}.DepthStencil), "multisample": unsafe.Offsetof(WRenderPipelineDescriptor{}.Multisample), "fragment": unsafe.Offsetof(WRenderPipelineDescriptor{}.Fragment)}},
		"WGPUSamplerDescriptor":                {size: unsafe.Sizeof(WSamplerDescriptor{}), fields: map[string]uintptr{"addressModeU": unsafe.Offsetof(WSamplerDescriptor{}.AddressU), "addressModeV": unsafe.Offsetof(WSamplerDescriptor{}.AddressV), "magFilter": unsafe.Offsetof(WSamplerDescriptor{}.MagFilter), "minFilter": unsafe.Offsetof(WSamplerDescriptor{}.MinFilter), "mipmapFilter": unsafe.Offsetof(WSamplerDescriptor{}.MipmapFilter), "lodMinClamp": unsafe.Offsetof(WSamplerDescriptor{}.LodMin), "lodMaxClamp": unsafe.Offsetof(WSamplerDescriptor{}.LodMax), "compare": unsafe.Offsetof(WSamplerDescriptor{}.Compare), "maxAnisotropy": unsafe.Offsetof(WSamplerDescriptor{}.MaxAniso)}},
		"WGPUBufferDescriptor":                 {size: unsafe.Sizeof(WBufferDescriptor{}), fields: map[string]uintptr{"usage": unsafe.Offsetof(WBufferDescriptor{}.Usage), "size": unsafe.Offsetof(WBufferDescriptor{}.Size), "mappedAtCreation": unsafe.Offsetof(WBufferDescriptor{}.MappedAtCreation)}},
		"WGPUTextureDescriptor":                {size: unsafe.Sizeof(WTextureDescriptor{}), fields: map[string]uintptr{"usage": unsafe.Offsetof(WTextureDescriptor{}.Usage), "dimension": unsafe.Offsetof(WTextureDescriptor{}.Dimension), "size": unsafe.Offsetof(WTextureDescriptor{}.Size), "format": unsafe.Offsetof(WTextureDescriptor{}.Format), "mipLevelCount": unsafe.Offsetof(WTextureDescriptor{}.MipLevelCount), "sampleCount": unsafe.Offsetof(WTextureDescriptor{}.SampleCount), "viewFormatCount": unsafe.Offsetof(WTextureDescriptor{}.ViewFormatCount), "viewFormats": unsafe.Offsetof(WTextureDescriptor{}.ViewFormats)}},
		"WGPUCommandEncoderDescriptor":         {size: unsafe.Sizeof(WCommandEncoderDescriptor{}), fields: map[string]uintptr{"label": unsafe.Offsetof(WCommandEncoderDescriptor{}.Label)}},
		"WGPURenderPassColorAttachment":        {size: unsafe.Sizeof(WRenderPassColorAttachment{}), fields: map[string]uintptr{"view": unsafe.Offsetof(WRenderPassColorAttachment{}.View), "depthSlice": unsafe.Offsetof(WRenderPassColorAttachment{}.DepthSlice), "resolveTarget": unsafe.Offsetof(WRenderPassColorAttachment{}.ResolveTarget), "loadOp": unsafe.Offsetof(WRenderPassColorAttachment{}.LoadOp), "storeOp": unsafe.Offsetof(WRenderPassColorAttachment{}.StoreOp), "clearValue": unsafe.Offsetof(WRenderPassColorAttachment{}.ClearValue)}},
		"WGPURenderPassDepthStencilAttachment": {size: unsafe.Sizeof(WRenderPassDepthStencilAttachment{}), fields: map[string]uintptr{"view": unsafe.Offsetof(WRenderPassDepthStencilAttachment{}.View), "depthLoadOp": unsafe.Offsetof(WRenderPassDepthStencilAttachment{}.DepthLoadOp), "depthStoreOp": unsafe.Offsetof(WRenderPassDepthStencilAttachment{}.DepthStoreOp), "depthClearValue": unsafe.Offsetof(WRenderPassDepthStencilAttachment{}.DepthClearValue), "depthReadOnly": unsafe.Offsetof(WRenderPassDepthStencilAttachment{}.DepthReadOnly), "stencilLoadOp": unsafe.Offsetof(WRenderPassDepthStencilAttachment{}.StencilLoadOp), "stencilStoreOp": unsafe.Offsetof(WRenderPassDepthStencilAttachment{}.StencilStoreOp), "stencilClearValue": unsafe.Offsetof(WRenderPassDepthStencilAttachment{}.StencilClearValue), "stencilReadOnly": unsafe.Offsetof(WRenderPassDepthStencilAttachment{}.StencilReadOnly)}},
		"WGPURenderPassDescriptor":             {size: unsafe.Sizeof(WRenderPassDescriptor{}), fields: map[string]uintptr{"colorAttachmentCount": unsafe.Offsetof(WRenderPassDescriptor{}.ColorCount), "colorAttachments": unsafe.Offsetof(WRenderPassDescriptor{}.Colors), "depthStencilAttachment": unsafe.Offsetof(WRenderPassDescriptor{}.DepthStencil)}},
		"WGPUTexelCopyTextureInfo":             {size: unsafe.Sizeof(WTexelCopyTextureInfo{}), fields: map[string]uintptr{"texture": unsafe.Offsetof(WTexelCopyTextureInfo{}.Texture), "mipLevel": unsafe.Offsetof(WTexelCopyTextureInfo{}.MipLevel), "origin": unsafe.Offsetof(WTexelCopyTextureInfo{}.Origin), "aspect": unsafe.Offsetof(WTexelCopyTextureInfo{}.Aspect)}},
		"WGPUTexelCopyBufferInfo":              {size: unsafe.Sizeof(WTexelCopyBufferInfo{}), fields: map[string]uintptr{"layout": unsafe.Offsetof(WTexelCopyBufferInfo{}.Layout), "buffer": unsafe.Offsetof(WTexelCopyBufferInfo{}.Buffer)}},
		"WGPUTexelCopyBufferLayout":            {size: unsafe.Sizeof(WTexelCopyBufferLayout{})},
		"WGPUCommandBufferDescriptor":          {size: unsafe.Sizeof(WCommandBufferDescriptor{})},
		"WGPUAdapterInfo":                      {size: unsafe.Sizeof(WAdapterInfo{}), fields: map[string]uintptr{"vendor": unsafe.Offsetof(WAdapterInfo{}.Vendor), "architecture": unsafe.Offsetof(WAdapterInfo{}.Architecture), "device": unsafe.Offsetof(WAdapterInfo{}.Device), "description": unsafe.Offsetof(WAdapterInfo{}.Description), "backendType": unsafe.Offsetof(WAdapterInfo{}.BackendType), "adapterType": unsafe.Offsetof(WAdapterInfo{}.AdapterType), "vendorID": unsafe.Offsetof(WAdapterInfo{}.VendorID), "deviceID": unsafe.Offsetof(WAdapterInfo{}.DeviceID)}},
		"WGPUBufferBindingLayout":              {size: unsafe.Sizeof(WBufferBindingLayout{}), fields: map[string]uintptr{"type": unsafe.Offsetof(WBufferBindingLayout{}.Type), "hasDynamicOffset": unsafe.Offsetof(WBufferBindingLayout{}.HasDynamicOff), "minBindingSize": unsafe.Offsetof(WBufferBindingLayout{}.MinBindingSize)}},
		"WGPUSamplerBindingLayout":             {size: unsafe.Sizeof(WSamplerBindingLayout{}), fields: map[string]uintptr{"type": unsafe.Offsetof(WSamplerBindingLayout{}.Type)}},
		"WGPUTextureBindingLayout":             {size: unsafe.Sizeof(WTextureBindingLayout{}), fields: map[string]uintptr{"sampleType": unsafe.Offsetof(WTextureBindingLayout{}.SampleType), "viewDimension": unsafe.Offsetof(WTextureBindingLayout{}.ViewDim), "multisampled": unsafe.Offsetof(WTextureBindingLayout{}.Multisampled)}},
		"WGPUStorageTextureBindingLayout":      {size: unsafe.Sizeof(WStorageTextureBindingLayout{}), fields: map[string]uintptr{"access": unsafe.Offsetof(WStorageTextureBindingLayout{}.Access), "format": unsafe.Offsetof(WStorageTextureBindingLayout{}.Format), "viewDimension": unsafe.Offsetof(WStorageTextureBindingLayout{}.ViewDim)}},
		"WGPUTextureViewDescriptor":            {size: unsafe.Sizeof(WTextureViewDescriptor{}), fields: map[string]uintptr{"format": unsafe.Offsetof(WTextureViewDescriptor{}.Format), "dimension": unsafe.Offsetof(WTextureViewDescriptor{}.Dimension), "baseMipLevel": unsafe.Offsetof(WTextureViewDescriptor{}.BaseMipLevel), "mipLevelCount": unsafe.Offsetof(WTextureViewDescriptor{}.MipLevelCount), "baseArrayLayer": unsafe.Offsetof(WTextureViewDescriptor{}.BaseArrayLayer), "arrayLayerCount": unsafe.Offsetof(WTextureViewDescriptor{}.ArrayLayerCount), "aspect": unsafe.Offsetof(WTextureViewDescriptor{}.Aspect), "usage": unsafe.Offsetof(WTextureViewDescriptor{}.Usage)}},
	}
	bad := 0
	for _, c := range checks {
		g, ok := goStructs[c.name]
		if !ok {
			t.Errorf("%s: not in Go", c.name)
			bad++
			continue
		}
		if g.size != c.size {
			t.Errorf("%s: Go size %d, C size %d", c.name, g.size, c.size)
			bad++
		}
		for _, f := range c.fields {
			if g.fields == nil {
				continue
			}
			if off, ok := g.fields[f.name]; ok {
				if off != f.offset {
					t.Errorf("%s.%s: Go offset %d, C offset %d", c.name, f.name, off, f.offset)
					bad++
				}
			} else {
				t.Errorf("%s.%s: not in Go map", c.name, f.name)
				bad++
			}
		}
	}
	if bad > 0 {
		t.Fatalf("%d mismatches", bad)
	}
}
