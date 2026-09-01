// Lit stand-in for the exported glbs' unlit retail materials (cutout /
// opaque half). The retail shading survives: COLOR_0 (the baked packet
// colour) still multiplies the texture exactly as the unlit export does -
// this shader only adds a Standard-lighting response on top, so the scene
// keeps its palette under the realism sun.
//
// Two source-data quirks this shader absorbs:
// - the glbs carry NO normals; the builder generates smoothed ones on a
//   duplicated mesh (LegaiaRealism.SmoothedCopy) before assigning this
//   shader, and RecalculateTangents runs there because writing o.Normal
//   below puts the surface shader on the tangent-space path;
// - the PSX source winding is MIXED (retail culled per-view via NCLIP),
//   so a per-vertex normal can only be canonicalised, not made to face
//   the camera. Cull Off + the VFACE flip lights whichever side is seen.
Shader "Legaia/Lit Vertex Color (Cutout)"
{
    Properties
    {
        _MainTex ("Base (RGB) Alpha (cutout)", 2D) = "white" {}
        _Color ("Tint", Color) = (1,1,1,1)
        _Cutoff ("Alpha cutoff", Range(0,1)) = 0.5
        _Glossiness ("Smoothness", Range(0,1)) = 0.05
    }
    SubShader
    {
        Tags { "RenderType"="TransparentCutout" "Queue"="AlphaTest" }
        Cull Off
        LOD 200

        CGPROGRAM
        #pragma surface surf Standard alphatest:_Cutoff addshadow fullforwardshadows
        #pragma target 3.0

        sampler2D _MainTex;
        fixed4 _Color;
        half _Glossiness;

        struct Input
        {
            float2 uv_MainTex;
            float4 color : COLOR;
            fixed facing : VFACE;
        };

        void surf (Input IN, inout SurfaceOutputStandard o)
        {
            fixed4 t = tex2D(_MainTex, IN.uv_MainTex) * _Color;
            o.Albedo = t.rgb * IN.color.rgb;
            o.Alpha = t.a * IN.color.a;
            o.Metallic = 0;
            o.Smoothness = _Glossiness;
            // Light the visible side regardless of the mixed source winding.
            o.Normal = float3(0, 0, IN.facing >= 0 ? 1 : -1);
        }
        ENDCG
    }
    FallBack "Legacy Shaders/Transparent/Cutout/VertexLit"
}
