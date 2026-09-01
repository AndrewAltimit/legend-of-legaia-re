// Lit stand-in for the exporter's BLEND materials (ABE semi-transparent
// prims: water sheets, light pools - split at half alpha by the export).
// Same contract as the cutout sibling: COLOR_0 keeps modulating the
// texture, lighting is layered on top, and the vert modifier flips the
// smoothed normal toward the camera per vertex (the mixed PSX winding
// makes VFACE the wrong signal - see the cutout shader's header).
// alpha:fade keeps depth writes off, which is also what keeps retail's
// coincident water scroll layers from z-fighting.
Shader "Legaia/Lit Vertex Color (Transparent)"
{
    Properties
    {
        _MainTex ("Base (RGB) Alpha (blend)", 2D) = "white" {}
        _Color ("Tint", Color) = (1,1,1,1)
        _Glossiness ("Smoothness", Range(0,1)) = 0.3
    }
    SubShader
    {
        Tags { "RenderType"="Transparent" "Queue"="Transparent" }
        Cull Off
        LOD 200

        CGPROGRAM
        #pragma surface surf Standard vertex:vert alpha:fade
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
    FallBack "Legacy Shaders/Transparent/VertexLit"
}
