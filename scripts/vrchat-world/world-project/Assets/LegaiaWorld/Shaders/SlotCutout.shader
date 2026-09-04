// Unlit alpha-cutout for the slot machine's screen composition. Two ways
// this differs from the legacy "Unlit/Transparent Cutout" it replaces:
//
// - Cull Off: the composition root carries a negative x scale (the single
//   mirror that makes a +z-facing quad read correctly to the player), which
//   reverses triangle winding - back-culled quads would all vanish.
// - _Color tint, so the black reel backdrop is this same shader with a
//   black tint instead of a second shader.
//
// The builder sets material.renderQueue per overlay layer: the composition
// is z-flattened onto the cabinet's screen face, so layer order is decided
// by queue (later draws win via ZTest LEqual at equal depth), not by the
// micrometre z gaps the flatten leaves behind.
Shader "LegaiaWorld/SlotCutout"
{
    Properties
    {
        _MainTex ("Texture", 2D) = "white" {}
        _Color ("Tint", Color) = (1, 1, 1, 1)
        _Cutoff ("Alpha cutoff", Range(0, 1)) = 0.5
    }
    SubShader
    {
        Tags { "Queue" = "AlphaTest" "RenderType" = "TransparentCutout" }
        Lighting Off
        Cull Off
        Pass
        {
            CGPROGRAM
            #pragma vertex vert
            #pragma fragment frag
            #include "UnityCG.cginc"

            sampler2D _MainTex;
            float4 _MainTex_ST;
            fixed4 _Color;
            float _Cutoff;

            struct appdata
            {
                float4 vertex : POSITION;
                float2 uv : TEXCOORD0;
            };

            struct v2f
            {
                float4 pos : SV_POSITION;
                float2 uv : TEXCOORD0;
            };

            v2f vert(appdata v)
            {
                v2f o;
                o.pos = UnityObjectToClipPos(v.vertex);
                o.uv = TRANSFORM_TEX(v.uv, _MainTex);
                return o;
            }

            fixed4 frag(v2f i) : SV_Target
            {
                fixed4 c = tex2D(_MainTex, i.uv) * _Color;
                clip(c.a - _Cutoff);
                return c;
            }
            ENDCG
        }
    }
    Fallback "Unlit/Transparent Cutout"
}
