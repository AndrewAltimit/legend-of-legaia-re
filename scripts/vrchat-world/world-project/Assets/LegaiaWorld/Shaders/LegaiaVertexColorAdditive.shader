// Additive stand-in for the exporter's `legaia_semi_abr1` / `abr3` / `abr2`
// materials. Core glTF can only say "alpha blend", so the export ships those
// prims as BLEND at half alpha and names the material with its real PSX ABR
// rate; the realism pass routes the named materials here. PSX rate 1 is
// `B + F` (pure additive - window light shafts, glow cones), rate 3 is
// `B + F/4` (same equation at quarter strength - _Intensity 0.25), rate 2 is
// `B - F` (_BlendOp ReverseSubtract). Rendered as plain alpha blend these
// read as a grey film over the scene; additive only ever brightens, which is
// the retail look.
//
// Unlit on purpose: retail applies no light source to these prims (the
// texel * packet-colour product IS the glow), and additive geometry is
// self-luminous - running it through the lit shaders would double-darken it
// at night. Fog fades the contribution to nothing (black) instead of
// tinting it toward the fog colour, the standard additive-fog treatment.
Shader "Legaia/Vertex Color (Additive)"
{
    Properties
    {
        _MainTex ("Base (RGB)", 2D) = "white" {}
        _Color ("Tint", Color) = (1,1,1,1)
        // PSX blend-rate weight: 1 = B + F (rate 1), 0.25 = B + F/4 (rate 3).
        _Intensity ("Intensity", Range(0,1)) = 1
        // Add for rates 1/3; ReverseSubtract (2) for PSX rate 2 (B - F).
        [Enum(UnityEngine.Rendering.BlendOp)] _BlendOp ("Blend op", Float) = 0
    }
    SubShader
    {
        Tags { "RenderType"="Transparent" "Queue"="Transparent" }
        Cull Off
        ZWrite Off
        BlendOp [_BlendOp]
        Blend One One

        Pass
        {
            CGPROGRAM
            #pragma vertex vert
            #pragma fragment frag
            #pragma multi_compile_fog
            #include "UnityCG.cginc"

            sampler2D _MainTex;
            float4 _MainTex_ST;
            fixed4 _Color;
            half _Intensity;

            struct appdata
            {
                float4 vertex : POSITION;
                float2 uv : TEXCOORD0;
                fixed4 color : COLOR;
            };

            struct v2f
            {
                float4 pos : SV_POSITION;
                float2 uv : TEXCOORD0;
                fixed4 color : COLOR;
                UNITY_FOG_COORDS(1)
            };

            v2f vert (appdata v)
            {
                v2f o;
                o.pos = UnityObjectToClipPos(v.vertex);
                o.uv = TRANSFORM_TEX(v.uv, _MainTex);
                o.color = v.color;
                UNITY_TRANSFER_FOG(o, o.pos);
                return o;
            }

            fixed4 frag (v2f i) : SV_Target
            {
                fixed4 t = tex2D(_MainTex, i.uv) * _Color;
                // Fully-transparent atlas texels (BGR555 word 0) are black
                // already; the alpha multiply keeps any filtered edge dark
                // so it adds nothing.
                fixed4 c;
                c.rgb = t.rgb * i.color.rgb * t.a * _Intensity;
                c.a = 1;
                // Additive fades to black in fog, never toward fog colour.
                UNITY_APPLY_FOG_COLOR(i.fogCoord, c, fixed4(0, 0, 0, 0));
                return c;
            }
            ENDCG
        }
    }
    FallBack Off
}
