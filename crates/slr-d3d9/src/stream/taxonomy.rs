//! Classification of every D3D9 entry point the logs may carry. A method that
//! is not listed is an error, never silently ignored.

/// What a method contributes to the canonical stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// Mutates device state; visible through later draw/clear snapshots.
    State,
    Draw,
    Scene,
    Clear,
    Present,
    /// Surface/texture copies and fills (ordered events).
    Transfer,
    /// Resource creation (ordered events).
    Create,
    /// Lock/unlock/preload/LOD: payload movement the log does not carry.
    ResourceData,
    /// Read-only queries, capability checks and enumeration.
    Query,
    /// Instance and device creation, cursor and dialog configuration.
    Env,
    /// `Reset`: all state returns to defaults.
    Reset,
    /// Known methods whose semantics the canonicalizer does not model; a log
    /// containing one is rejected rather than compared.
    Unsupported,
}

impl Class {
    pub fn name(self) -> &'static str {
        match self {
            Class::State => "state",
            Class::Draw => "draw",
            Class::Scene => "scene",
            Class::Clear => "clear",
            Class::Present => "present",
            Class::Transfer => "transfer",
            Class::Create => "create",
            Class::ResourceData => "resource_data",
            Class::Query => "query",
            Class::Env => "env",
            Class::Reset => "reset",
            Class::Unsupported => "unsupported",
        }
    }
}

pub fn classify(method: &str) -> Option<Class> {
    if let Some((interface, suffix)) = method.split_once('.') {
        if !interface.starts_with("IDirect3D") {
            return None;
        }
        return match suffix {
            "Lock"
            | "Unlock"
            | "LockRect"
            | "UnlockRect"
            | "LockBox"
            | "UnlockBox"
            | "PreLoad"
            | "SetLOD"
            | "SetPriority"
            | "GenerateMipSubLevels"
            | "SetAutoGenTextureFilter"
            | "SetPrivateData"
            | "FreePrivateData" => Some(Class::ResourceData),
            "QueryInterface" | "Issue" => Some(Class::Query),
            s if s.starts_with("Get") => Some(Class::Query),
            _ => None,
        };
    }
    Some(match method {
        "SetRenderState"
        | "SetTextureStageState"
        | "SetSamplerState"
        | "SetTransform"
        | "MultiplyTransform"
        | "SetViewport"
        | "SetMaterial"
        | "SetLight"
        | "LightEnable"
        | "SetClipPlane"
        | "SetScissorRect"
        | "SetFVF"
        | "SetTexture"
        | "SetStreamSource"
        | "SetStreamSourceFreq"
        | "SetIndices"
        | "SetVertexShader"
        | "SetPixelShader"
        | "SetVertexDeclaration"
        | "SetVertexShaderConstantF"
        | "SetVertexShaderConstantI"
        | "SetVertexShaderConstantB"
        | "SetPixelShaderConstantF"
        | "SetPixelShaderConstantI"
        | "SetPixelShaderConstantB"
        | "SetRenderTarget"
        | "SetDepthStencilSurface"
        | "SetClipStatus"
        | "SetPaletteEntries"
        | "SetCurrentTexturePalette"
        | "SetNPatchMode"
        | "SetSoftwareVertexProcessing" => Class::State,
        "DrawPrimitive" | "DrawIndexedPrimitive" | "DrawPrimitiveUP" | "DrawIndexedPrimitiveUP" => {
            Class::Draw
        }
        "BeginScene" | "EndScene" => Class::Scene,
        "Clear" => Class::Clear,
        "Present" => Class::Present,
        "StretchRect" | "ColorFill" | "UpdateSurface" | "UpdateTexture" => Class::Transfer,
        "CreateTexture"
        | "CreateVolumeTexture"
        | "CreateCubeTexture"
        | "CreateVertexBuffer"
        | "CreateIndexBuffer"
        | "CreateRenderTarget"
        | "CreateDepthStencilSurface"
        | "CreateOffscreenPlainSurface"
        | "CreateVertexDeclaration"
        | "CreateVertexShader"
        | "CreatePixelShader"
        | "CreateQuery" => Class::Create,
        "Reset" => Class::Reset,
        "Direct3DCreate9"
        | "CreateDevice"
        | "RegisterSoftwareDevice"
        | "SetCursorProperties"
        | "SetCursorPosition"
        | "ShowCursor"
        | "SetDialogBoxMode"
        | "SetGammaRamp" => Class::Env,
        "TestCooperativeLevel"
        | "GetAvailableTextureMem"
        | "EvictManagedResources"
        | "GetDirect3D"
        | "GetDeviceCaps"
        | "GetDisplayMode"
        | "GetCreationParameters"
        | "GetSwapChain"
        | "GetNumberOfSwapChains"
        | "GetBackBuffer"
        | "GetRasterStatus"
        | "GetGammaRamp"
        | "GetRenderTargetData"
        | "GetFrontBufferData"
        | "GetRenderTarget"
        | "GetDepthStencilSurface"
        | "GetTransform"
        | "GetViewport"
        | "GetMaterial"
        | "GetLight"
        | "GetLightEnable"
        | "GetClipPlane"
        | "GetRenderState"
        | "GetClipStatus"
        | "GetTexture"
        | "GetTextureStageState"
        | "GetSamplerState"
        | "ValidateDevice"
        | "GetPaletteEntries"
        | "GetCurrentTexturePalette"
        | "GetScissorRect"
        | "GetSoftwareVertexProcessing"
        | "GetNPatchMode"
        | "GetVertexDeclaration"
        | "GetFVF"
        | "GetVertexShader"
        | "GetVertexShaderConstantF"
        | "GetVertexShaderConstantI"
        | "GetVertexShaderConstantB"
        | "GetStreamSource"
        | "GetStreamSourceFreq"
        | "GetIndices"
        | "GetPixelShader"
        | "GetPixelShaderConstantF"
        | "GetPixelShaderConstantI"
        | "GetPixelShaderConstantB"
        | "GetAdapterCount"
        | "GetAdapterIdentifier"
        | "GetAdapterModeCount"
        | "EnumAdapterModes"
        | "GetAdapterDisplayMode"
        | "CheckDeviceType"
        | "CheckDeviceFormat"
        | "CheckDeviceMultiSampleType"
        | "CheckDepthStencilMatch"
        | "CheckDeviceFormatConversion"
        | "GetAdapterMonitor" => Class::Query,
        "ProcessVertices"
        | "DrawRectPatch"
        | "DrawTriPatch"
        | "DeletePatch"
        | "CreateStateBlock"
        | "BeginStateBlock"
        | "EndStateBlock"
        | "CreateAdditionalSwapChain" => Class::Unsupported,
        _ => return None,
    })
}

/// The `kind` byte the hook layer / headless oracle assign to a D3D9 record
/// (`startup.py::d3d9_kind`): 0 state, 1 draw, 2 resource, 3 vertex format,
/// 4 present. Mirrored so the port's emitter produces identical framing.
pub fn log_kind(method: &str) -> u8 {
    if method.starts_with("Draw") || method.starts_with("Clear") {
        return 1;
    }
    if method == "Present" || method == "PresentEx" {
        return 4;
    }
    if method.contains("FVF") || method.contains("VertexDeclaration") {
        return 3;
    }
    const RESOURCE_WORDS: [&str; 22] = [
        "Create",
        "Lock",
        "Unlock",
        "StreamSource",
        "Indices",
        "SetTexture",
        "StretchRect",
        "Update",
        "PreLoad",
        "RenderTarget",
        "DepthStencil",
        "GetBackBuffer",
        "ColorFill",
        "Release",
        "SetPrivateData",
        "GenerateMipSubLevels",
        "SetAutoGenTextureFilter",
        "LockRect",
        "UnlockRect",
        "LockBox",
        "UnlockBox",
        "GetSurfaceLevel",
    ];
    if RESOURCE_WORDS
        .iter()
        .any(|w| method.starts_with(w) || method.contains(w))
    {
        return 2;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_methods_classify_and_unknown_fails_closed() {
        assert_eq!(classify("SetRenderState"), Some(Class::State));
        assert_eq!(classify("DrawIndexedPrimitive"), Some(Class::Draw));
        assert_eq!(
            classify("IDirect3DVertexBuffer9.Lock"),
            Some(Class::ResourceData)
        );
        assert_eq!(
            classify("IDirect3DTexture9.GetLevelDesc"),
            Some(Class::Query)
        );
        assert_eq!(classify("DoSomethingNew"), None);
        assert_eq!(classify("IDirect3DTexture9.Frobnicate"), None);
        assert_eq!(classify("BeginStateBlock"), Some(Class::Unsupported));
    }

    #[test]
    fn log_kind_matches_the_oracle_rules() {
        assert_eq!(log_kind("Clear"), 1);
        assert_eq!(log_kind("DrawPrimitiveUP"), 1);
        assert_eq!(log_kind("Present"), 4);
        assert_eq!(log_kind("SetFVF"), 3);
        assert_eq!(log_kind("SetVertexDeclaration"), 3);
        assert_eq!(log_kind("SetTexture"), 2);
        assert_eq!(log_kind("CreateTexture"), 2);
        assert_eq!(log_kind("IDirect3DVertexBuffer9.Lock"), 2);
        assert_eq!(log_kind("SetRenderState"), 0);
        assert_eq!(log_kind("SetTransform"), 0);
        assert_eq!(log_kind("BeginScene"), 0);
    }
}
