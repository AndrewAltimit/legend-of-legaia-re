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
// at night. Fog fades the contribution to nothing (black additive, neutral
// multiplier) instead of tinting toward the fog colour.
//
// Blend space: the PSX GPU adds in DISPLAY (gamma) space, and so do the
// site's WebGL viewers - but a Linear-colour-space Unity project (VRChat
// mandates Linear) blends in linear. A linear-space `One One` add is both
// dimmer over any non-black background AND crushes the background detail
// seen THROUGH the prim: retail's display-space add has slope 1 in the
// background (the floor planks keep their full contrast behind the shaft),
// while a linear add transmits only a few percent of dark-background
// contrast - the shaft reads as an opaque milky film. No single
// fixed-function blend in linear space can express `srgb(B) + F_d`, so this
// shader splits it into two passes, a first-order fit in the (unreadable)
// destination:
//
//   pass 1  (Blend DstColor One):   B' = B * M      - the multiplicative
//           term carries the background through at boosted contrast;
//   pass 2  (Blend One One):        R  = B' + A     - the additive floor,
//           exact over a black background.
//
// Per pixel, A = linear(F_d) and M is fitted so the result is exact at a
// reference mid-dark background (B_REF, display 0.25 - the tone range these
// prims sit over) as well as at black; brighter backgrounds saturate to
// white slightly early, which is where retail clips too.
Shader "Legaia/Vertex Color (Additive)"
{
    Properties
    {
        _MainTex ("Base (RGB)", 2D) = "white" {}
        _Color ("Tint", Color) = (1,1,1,1)
        // PSX blend-rate weight: 1 = B + F (rate 1), 0.25 = B + F/4 (rate 3).
        _Intensity ("Intensity", Range(0,1)) = 1
        // Extra user brightness trim on top of the display-space conversion.
        _Boost ("Brightness boost", Range(0.25, 2)) = 1
        // Add for rates 1/3; ReverseSubtract (2) for PSX rate 2 (B - F).
        // A non-Add op also disables the multiplicative pass (a subtractive
        // prim must not brighten its background first).
        [Enum(UnityEngine.Rendering.BlendOp)] _BlendOp ("Blend op", Float) = 0
    }
    SubShader
    {
        Tags { "RenderType"="Transparent" "Queue"="Transparent" }
        Cull Off
        ZWrite Off

        CGINCLUDE
        #include "UnityCG.cginc"

        sampler2D _MainTex;
        float4 _MainTex_ST;
        fixed4 _Color;
        half _Intensity;
        half _Boost;
        half _BlendOp;

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

        // The prim's DISPLAY-space contribution F_d (what the PSX adds), and
        // its linear additive floor A. Fully-transparent atlas texels
        // (BGR555 word 0) are black already; the alpha multiply keeps any
        // filtered edge dark so it contributes nothing.
        void contribution (v2f i, out half3 fd, out half3 add)
        {
            fixed4 t = tex2D(_MainTex, i.uv) * _Color;
            half3 f = max(t.rgb * i.color.rgb * t.a, 0);
            #ifndef UNITY_COLORSPACE_GAMMA
            f = LinearToGammaSpace(f);
            #endif
            // _Intensity is the PSX rate weight (B + F/4 scales the
            // DISPLAY-space term), so it applies after the lift.
            fd = f * _Intensity * _Boost;
            #ifndef UNITY_COLORSPACE_GAMMA
            add = GammaToLinearSpace(fd);
            #else
            add = fd;
            #endif
        }
        ENDCG

        // Pass 1: multiply the background by M, carrying its detail through
        // the prim. Outputs M - 1 (Blend DstColor One computes B*(out+1)).
        Pass
        {
            Blend DstColor One
            CGPROGRAM
            #pragma vertex vert
            #pragma fragment frag
            #pragma multi_compile_fog

            // Display value of the reference background the fit is exact at,
            // and its linear form.
            #define B_REF 0.25
            #define B_REF_LIN 0.0508

            fixed4 frag (v2f i) : SV_Target
            {
                half3 fd, add;
                contribution(i, fd, add);
                fixed4 c;
                #ifndef UNITY_COLORSPACE_GAMMA
                // M = (linear(B_REF + F_d) - A) / linear(B_REF): exact at
                // the reference background, monotone in F_d, and exactly 1
                // (this pass a no-op) when F_d = 0.
                half3 m = (GammaToLinearSpace(saturate(B_REF + fd)) - add)
                    / B_REF_LIN;
                c.rgb = max(m - 1, 0);
                #else
                // A Gamma project blends in display space already - the
                // plain additive pass below is exact on its own.
                c.rgb = 0;
                #endif
                if (_BlendOp != 0)
                    c.rgb = 0; // subtractive prims keep the single-pass math
                c.a = 1;
                // Fog: fade toward the neutral multiplier (M = 1).
                UNITY_APPLY_FOG_COLOR(i.fogCoord, c, fixed4(0, 0, 0, 0));
                return c;
            }
            ENDCG
        }

        // Pass 2: the additive floor (exact over black). BlendOp RevSub
        // turns this into the PSX subtractive mode for rate-2 prims.
        Pass
        {
            BlendOp [_BlendOp]
            Blend One One
            CGPROGRAM
            #pragma vertex vert
            #pragma fragment frag
            #pragma multi_compile_fog

            fixed4 frag (v2f i) : SV_Target
            {
                half3 fd, add;
                contribution(i, fd, add);
                fixed4 c;
                c.rgb = add;
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
