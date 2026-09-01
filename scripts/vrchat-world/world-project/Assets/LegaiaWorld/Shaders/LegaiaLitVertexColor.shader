// Lit stand-in for the exported glbs' unlit retail materials (cutout /
// opaque half). The retail shading survives: COLOR_0 (the baked packet
// colour) still multiplies the texture exactly as the unlit export does -
// this shader only adds a lighting response on top, so the scene keeps
// its palette under the realism sun.
//
// Lighting is a two-sided Lambert: |N.L|, sign-independent. That is the
// third design here, and the only stable one - keep the history so it
// isn't re-walked:
// - the glbs carry NO normals; the builder generates smoothed ones on a
//   duplicated mesh (LegaiaRealism.SmoothedCopy). The PSX source winding
//   is MIXED (retail culled per-view via NCLIP) and the import stack
//   layers mirrors on top, so the generated normal's SIGN is only locally
//   consistent - no global "outward" exists on this data.
// - a VFACE flip follows the (random) winding: mixed-wound ground turned
//   into random black patches, zigzag walls into lit/dark stripes.
// - a flip toward the camera fixed those, but is unstable exactly at
//   grazing view (dot(N, toCam) ~ 0 - i.e. looking along a wall), where
//   per-vertex sign noise re-created alternating strips per segment.
// - |N.L| needs no sign at all: nothing to decide, nothing to flicker.
//   The cost - a thin wall's shadow side is as bright as its sun side -
//   reads closer to the unlit retail look than a dramatic dark side, and
//   shadow-map attenuation still darkens genuinely occluded surfaces.
Shader "Legaia/Lit Vertex Color (Cutout)"
{
    Properties
    {
        _MainTex ("Base (RGB) Alpha (cutout)", 2D) = "white" {}
        _Color ("Tint", Color) = (1,1,1,1)
        _Cutoff ("Alpha cutoff", Range(0,1)) = 0.5
        // 0 = full |N.L| (world surfaces). Toward 1 the angular term
        // flattens to even lighting - the realism pass raises it on
        // character/prop meshes, where the |N.L| terminator cuts a harsh
        // dark band right across a low-poly face. Shadow-map attenuation
        // still applies at any wrap.
        _LightWrap ("Light wrap (flatten shading)", Range(0,1)) = 0
    }
    SubShader
    {
        Tags { "RenderType"="TransparentCutout" "Queue"="AlphaTest" }
        Cull Off
        LOD 200

        CGPROGRAM
        #pragma surface surf LegaiaTwoSided alphatest:_Cutoff addshadow fullforwardshadows
        #pragma target 3.0

        sampler2D _MainTex;
        fixed4 _Color;
        half _LightWrap;

        struct Input
        {
            float2 uv_MainTex;
            float4 color : COLOR;
        };

        half4 LightingLegaiaTwoSided (SurfaceOutput s, half3 lightDir, half atten)
        {
            half nl = lerp(abs(dot(s.Normal, lightDir)), 1.0h, _LightWrap);
            half4 c;
            c.rgb = s.Albedo * _LightColor0.rgb * nl * atten;
            c.a = s.Alpha;
            return c;
        }

        void surf (Input IN, inout SurfaceOutput o)
        {
            fixed4 t = tex2D(_MainTex, IN.uv_MainTex) * _Color;
            o.Albedo = t.rgb * IN.color.rgb;
            o.Alpha = t.a * IN.color.a;
        }
        ENDCG
    }
    FallBack "Legacy Shaders/Transparent/Cutout/VertexLit"
}
