// Lit stand-in for the exported glbs' unlit retail materials (cutout /
// opaque half). The retail shading survives: COLOR_0 (the baked packet
// colour) still multiplies the texture exactly as the unlit export does -
// this shader only adds a Standard-lighting response on top, so the scene
// keeps its palette under the realism sun.
//
// Two source-data quirks this shader absorbs:
// - the glbs carry NO normals; the builder generates smoothed ones on a
//   duplicated mesh (LegaiaRealism.SmoothedCopy);
// - the PSX source winding is MIXED (retail culled per-view via NCLIP)
//   and the import stack layers several mirrors on top, so neither the
//   winding nor the stored normal sign says which way a surface really
//   faces. The vert modifier flips the smoothed normal toward the camera
//   per vertex, so both sides of every surface light as "front" - do NOT
//   swap this for a VFACE flip: VFACE follows the (random) winding, which
//   turns mixed-wound ground into random black patches and zigzag walls
//   into alternating lit/dark stripes.
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
        #pragma surface surf Standard vertex:vert alphatest:_Cutoff addshadow fullforwardshadows
        #pragma target 3.0

        sampler2D _MainTex;
        fixed4 _Color;
        half _Glossiness;

        struct Input
        {
            float2 uv_MainTex;
            float4 color : COLOR;
        };

        void vert (inout appdata_full v)
        {
            // Face the smoothed normal toward the viewer (see header).
            float3 wpos = mul(unity_ObjectToWorld, v.vertex).xyz;
            float3 wn = UnityObjectToWorldNormal(v.normal);
            if (dot(wn, _WorldSpaceCameraPos - wpos) < 0)
                v.normal = -v.normal;
        }

        void surf (Input IN, inout SurfaceOutputStandard o)
        {
            fixed4 t = tex2D(_MainTex, IN.uv_MainTex) * _Color;
            o.Albedo = t.rgb * IN.color.rgb;
            o.Alpha = t.a * IN.color.a;
            o.Metallic = 0;
            o.Smoothness = _Glossiness;
        }
        ENDCG
    }
    FallBack "Legacy Shaders/Transparent/Cutout/VertexLit"
}
