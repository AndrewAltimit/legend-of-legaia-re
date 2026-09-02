// Lit stand-in for the exporter's BLEND materials (ABE semi-transparent
// prims: water sheets, light pools - split at half alpha by the export).
// Same contract as the cutout sibling: COLOR_0 keeps modulating the
// texture, and lighting is the sign-independent two-sided Lambert |N.L|
// (see the cutout shader's header for why neither a VFACE flip nor a
// toward-camera flip survives this data's mixed winding).
// alpha:fade keeps depth writes off, which is also what keeps retail's
// coincident water scroll layers from z-fighting.
Shader "Legaia/Lit Vertex Color (Transparent)"
{
    Properties
    {
        _MainTex ("Base (RGB) Alpha (blend)", 2D) = "white" {}
        _Color ("Tint", Color) = (1,1,1,1)
        // See the cutout sibling: 0 = full |N.L|; toward 1 the angular
        // term flattens (raised on character/prop meshes by the realism
        // pass to kill the terminator band on low-poly figures).
        _LightWrap ("Light wrap (flatten shading)", Range(0,1)) = 0
    }
    SubShader
    {
        Tags { "RenderType"="Transparent" "Queue"="Transparent" }
        Cull Off
        LOD 200

        CGPROGRAM
        #pragma surface surf LegaiaTwoSided alpha:fade
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
    FallBack "Legacy Shaders/Transparent/VertexLit"
}
