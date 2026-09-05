// Reel-face shader for the casino slot machine: unlit alpha-cutout plus
// retail's depth-cued gouraud shade, computed per pixel.
//
// Retail (FUN_801d0fa8) shades every reel-face vertex from its model-space z:
//
//   shade = clamp(0xB4 - ((z + 0x200) * 0x21C >> 9), 0, 0xB4)
//   pixel = texel * shade / 128
//
// so the payline face (z = -0x200, toward the viewer) is brightened 1.41x and
// everything away from it fades to black - that fade IS the top/bottom cap of
// each reel window, and the reason the near half of the cylinder never shows
// (there is no backface cull). See docs/subsystems/minigame-slot-machine.md.
//
// The machine's model-space z is reconstructed from world space via two baked
// vectors: _ShadeOrigin (the reel drum centre, world) and _ShadeAxis (the
// world-space direction of model +z, scaled to model units per world unit).
// LegaiaSlotMachineBuilder bakes them at build time and LegaiaSlotMachine
// refreshes them once in Start(), so a dragged rig self-corrects in play.
Shader "LegaiaWorld/SlotReelFace"
{
    Properties
    {
        _MainTex ("Texture", 2D) = "white" {}
        _Cutoff ("Alpha cutoff", Range(0, 1)) = 0.5
        _ShadeOrigin ("Shade origin (world)", Vector) = (0, 0, 0, 0)
        _ShadeAxis ("Shade axis (world -> model z)", Vector) = (0, 0, 1, 0)
    }
    SubShader
    {
        Tags { "Queue" = "AlphaTest" "RenderType" = "TransparentCutout" }
        Lighting Off
        // The screen composition is x-mirrored at its root (one flip makes
        // a +z-facing quad read correctly to the player), which reverses
        // triangle winding - so nothing here may backface-cull.
        Cull Off
        Pass
        {
            CGPROGRAM
            #pragma vertex vert
            #pragma fragment frag
            #include "UnityCG.cginc"

            sampler2D _MainTex;
            float4 _MainTex_ST;
            float _Cutoff;
            float4 _ShadeOrigin;
            float4 _ShadeAxis;

            struct appdata
            {
                float4 vertex : POSITION;
                float2 uv : TEXCOORD0;
            };

            struct v2f
            {
                float4 pos : SV_POSITION;
                float2 uv : TEXCOORD0;
                float3 wp : TEXCOORD1;
            };

            v2f vert(appdata v)
            {
                v2f o;
                o.pos = UnityObjectToClipPos(v.vertex);
                o.uv = TRANSFORM_TEX(v.uv, _MainTex);
                o.wp = mul(unity_ObjectToWorld, v.vertex).xyz;
                return o;
            }

            fixed4 frag(v2f i) : SV_Target
            {
                fixed4 c = tex2D(_MainTex, i.uv);
                clip(c.a - _Cutoff);
                // Model-space z of this pixel (payline face sits at -0x200).
                float z = dot(i.wp - _ShadeOrigin.xyz, _ShadeAxis.xyz);
                // clamp(0xB4 - (z + 0x200) * 0x21C / 512, 0, 0xB4) / 128
                float shade =
                    clamp(180.0 - (z + 512.0) * (540.0 / 512.0), 0.0, 180.0) / 128.0;
                c.rgb *= shade;
                return c;
            }
            ENDCG
        }
    }
    Fallback "Unlit/Transparent Cutout"
}
