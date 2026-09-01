// Decorative window-light shaft for the interior shells: an additive,
// double-sided, depth-write-free quad with a soft procedural falloff -
// sin(pi * u) across the width so the edges feather out, and the vertex
// alpha (bright at the entry, faint at the floor) fades it along its
// length. No texture, nothing sampled - entirely generated. Cull Off makes
// it immune to the built root's mirror; the "Legaia/" name prefix keeps
// the lit-conversion pass from touching this material.
Shader "Legaia/Light Shaft"
{
    Properties
    {
        _Color ("Color", Color) = (1, 0.95, 0.78, 0.45)
    }
    SubShader
    {
        Tags { "RenderType"="Transparent" "Queue"="Transparent+10"
               "IgnoreProjector"="True" }
        Cull Off
        ZWrite Off
        Blend SrcAlpha One
        LOD 100

        Pass
        {
            CGPROGRAM
            #pragma vertex vert
            #pragma fragment frag
            #include "UnityCG.cginc"

            fixed4 _Color;

            struct appdata
            {
                float4 vertex : POSITION;
                float2 uv : TEXCOORD0;
                float4 color : COLOR;
            };

            struct v2f
            {
                float4 pos : SV_POSITION;
                float2 uv : TEXCOORD0;
                fixed4 color : COLOR;
            };

            v2f vert (appdata v)
            {
                v2f o;
                o.pos = UnityObjectToClipPos(v.vertex);
                o.uv = v.uv;
                o.color = v.color;
                return o;
            }

            fixed4 frag (v2f i) : SV_Target
            {
                fixed4 c = _Color;
                c.a *= sin(saturate(i.uv.x) * UNITY_PI) * i.color.a;
                return c;
            }
            ENDCG
        }
    }
}
