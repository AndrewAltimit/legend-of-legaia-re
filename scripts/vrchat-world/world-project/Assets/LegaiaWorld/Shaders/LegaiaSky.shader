// The kit's sky: a procedural dome LegaiaDayNight drives per frame through
// plain material properties (Udon can set a material's floats, colours and
// vectors; it cannot set shader globals, which is why every knob here is a
// property and not a global).
//
// Layers, back to front:
//   gradient   zenith / horizon / ground colours with a horizon haze band,
//              plus a sunset glow on the sun's side of the horizon
//              (_HorizonGlow, driven by the twilight bell)
//   stars      one star per cell of a 3D hash grid on the view direction,
//              rotated about _StarAxis by _StarAngle so the sky wheels with
//              the sun; twinkle from _Time; a faint galactic band; a
//              shooting star every ~23 s while _StarStrength is up
//   moon       a disc with a real phase - a second circle offset along the
//              moon's tangent cuts the shadow, 0 = new, 0.5 = full - with
//              earthshine on the dark side, faint maria, and a glow
//   sun        disc + two-lobe glow, hidden below the horizon
//   clouds     fbm value noise on a dome projection, cover from
//              _CloudCover (the weather layer's cloudiness), lit / shade
//              colours from the cycle, silver lining toward the sun, and
//              a grey-out of the whole sky as cover goes overcast
//
// Noise lattice is wrapped at 256 so the cloud offset can be wrapped at
// 256 too without a seam - the hash is exact for small integer inputs and
// drifts once the lattice coordinate grows past a few thousand.
//
// Stereo: the direction is the object-space vertex of Unity's skybox mesh
// taken to world space, per eye, with the SPS-I macros - the same shape
// as Unity's own Skybox/Procedural.
Shader "Legaia/Sky"
{
    Properties
    {
        [Header(Gradient)]
        _ZenithColor ("Zenith", Color) = (0.24, 0.45, 0.85, 1)
        _HorizonColor ("Horizon", Color) = (0.70, 0.80, 0.92, 1)
        _GroundColor ("Ground", Color) = (0.40, 0.42, 0.45, 1)
        _HorizonGlowColor ("Horizon glow (sun side)", Color) = (1, 0.45, 0.18, 1)
        _HorizonGlow ("Horizon glow strength", Range(0, 1)) = 0
        _Exposure ("Exposure", Range(0, 4)) = 1.1

        [Header(Sun)]
        _SunDir ("Sun direction (toward the sun)", Vector) = (0.5, 0.7, 0.5, 0)
        _SunColor ("Sun colour", Color) = (1, 0.95, 0.85, 1)
        _SunSize ("Sun disc radius (rad)", Range(0.005, 0.2)) = 0.03
        _SunGlow ("Sun glow", Range(0, 2)) = 0.6

        [Header(Moon)]
        _MoonDir ("Moon direction", Vector) = (-0.5, -0.7, -0.5, 0)
        _MoonColor ("Moon colour", Color) = (0.85, 0.9, 1, 1)
        _MoonSize ("Moon disc radius (rad)", Range(0.005, 0.2)) = 0.028
        _MoonPhase ("Moon phase (0 new, 0.5 full)", Range(0, 1)) = 0.5
        _MoonGlow ("Moon glow", Range(0, 2)) = 0

        [Header(Stars)]
        _StarStrength ("Stars", Range(0, 1)) = 0
        _StarAxis ("Star axis", Vector) = (1, 0, 0, 0)
        _StarAngle ("Star angle (rad)", Float) = 0

        [Header(Clouds)]
        _CloudCover ("Cloud cover", Range(0, 1)) = 0.3
        _CloudColor ("Cloud lit", Color) = (1, 1, 1, 1)
        _CloudShadeColor ("Cloud shade", Color) = (0.62, 0.66, 0.76, 1)
        _CloudOffset ("Cloud offset (xy)", Vector) = (0, 0, 0, 0)
        _CloudScale ("Cloud scale", Float) = 1.6
        _CloudDrift ("Built-in drift (units/s, no Udon needed)", Float) = 0.0025
    }
    SubShader
    {
        Tags { "Queue"="Background" "RenderType"="Background" "PreviewType"="Skybox" }
        Cull Off
        ZWrite Off

        Pass
        {
            CGPROGRAM
            #pragma vertex vert
            #pragma fragment frag
            #pragma target 3.0
            #include "UnityCG.cginc"

            fixed4 _ZenithColor, _HorizonColor, _GroundColor, _HorizonGlowColor;
            float _HorizonGlow, _Exposure;
            float4 _SunDir; fixed4 _SunColor; float _SunSize, _SunGlow;
            float4 _MoonDir; fixed4 _MoonColor; float _MoonSize, _MoonPhase, _MoonGlow;
            float _StarStrength; float4 _StarAxis; float _StarAngle;
            float _CloudCover; fixed4 _CloudColor, _CloudShadeColor;
            float4 _CloudOffset; float _CloudScale, _CloudDrift;

            struct appdata
            {
                float4 vertex : POSITION;
                UNITY_VERTEX_INPUT_INSTANCE_ID
            };

            struct v2f
            {
                float4 pos : SV_POSITION;
                float3 dir : TEXCOORD0;
                UNITY_VERTEX_OUTPUT_STEREO
            };

            v2f vert(appdata v)
            {
                v2f o;
                UNITY_SETUP_INSTANCE_ID(v);
                UNITY_INITIALIZE_OUTPUT(v2f, o);
                UNITY_INITIALIZE_VERTEX_OUTPUT_STEREO(o);
                o.pos = UnityObjectToClipPos(v.vertex);
                o.dir = mul((float3x3)unity_ObjectToWorld, v.vertex.xyz);
                return o;
            }

            // --- hashes / noise ------------------------------------------
            float hash12(float2 p)
            {
                p -= 256.0 * floor(p / 256.0);
                float3 p3 = frac(float3(p.xyx) * 0.1031);
                p3 += dot(p3, p3.yzx + 33.33);
                return frac((p3.x + p3.y) * p3.z);
            }

            float3 hash33(float3 p)
            {
                float3 p3 = frac(p * float3(0.1031, 0.1030, 0.0973));
                p3 += dot(p3, p3.yxz + 33.33);
                return frac((p3.xxy + p3.yxx) * p3.zyx);
            }

            float vnoise(float2 p)
            {
                float2 i = floor(p);
                float2 f = frac(p);
                float2 u = f * f * (3.0 - 2.0 * f);
                float a = hash12(i);
                float b = hash12(i + float2(1, 0));
                float c = hash12(i + float2(0, 1));
                float d = hash12(i + float2(1, 1));
                return lerp(lerp(a, b, u.x), lerp(c, d, u.x), u.y);
            }

            float fbm(float2 p)
            {
                float v = 0.0;
                float a = 0.5;
                // A slight rotation per octave keeps the lattice from
                // showing through as axis-aligned streaks.
                float2x2 r = float2x2(0.8, 0.6, -0.6, 0.8);
                [unroll]
                for (int i = 0; i < 4; i++)
                {
                    v += a * vnoise(p);
                    p = mul(r, p) * 2.03;
                    a *= 0.5;
                }
                return v;
            }

            float3 rotateAxis(float3 v, float3 axis, float ang)
            {
                float s, c;
                sincos(ang, s, c);
                return v * c + cross(axis, v) * s + axis * dot(axis, v) * (1.0 - c);
            }

            // --- layers ----------------------------------------------------
            float3 stars(float3 d, float3 sunDir)
            {
                if (_StarStrength <= 0.001 || d.y <= 0.0)
                    return 0;
                float3 axis = normalize(_StarAxis.xyz + float3(1e-4, 0, 0));
                float3 sd = rotateAxis(d, axis, _StarAngle);

                float3 g = sd * 48.0;
                float3 cell = floor(g);
                float3 h = hash33(cell);
                float3 h2 = hash33(cell + 17.3);
                float has = step(0.72, h2.x);
                float dist = length(g - cell - h);
                float core = smoothstep(0.10, 0.0, dist);
                float size = 0.6 + 0.9 * h2.y * h2.y;         // a few bright ones
                float twinkle = 0.65 + 0.35 * sin(_Time.y * (1.5 + 3.0 * h2.z) + h.z * 6.2832);
                float3 tint = lerp(float3(0.75, 0.85, 1.0), float3(1.0, 0.9, 0.75), h2.z);
                float3 col = tint * core * has * size * twinkle;

                // A faint galactic band across the star frame.
                float3 bandN = normalize(float3(0.35, 0.55, 0.76));
                float band = pow(1.0 - abs(dot(sd, bandN)), 10.0);
                band *= 0.5 + 0.5 * vnoise(sd.xz * 6.0 + sd.y * 4.0);
                col += float3(0.55, 0.62, 0.85) * band * 0.10;

                // A shooting star: one every 23 s, a 2 s streak.
                float T = _Time.y / 23.0;
                float k = floor(T);
                float f = frac(T);
                if (f < 0.09)
                {
                    float3 m = hash33(float3(k, k * 1.7 + 3.1, 5.3));
                    float az = m.x * 6.2832;
                    float el = 0.35 + m.y * 0.9;
                    float3 s = float3(cos(el) * cos(az), sin(el), cos(el) * sin(az));
                    float3 t0 = normalize(cross(s, float3(0, 1, 0)) + 1e-4);
                    float3 t1 = cross(s, t0);
                    float3 tng = t0 * cos(m.z * 6.2832) + t1 * sin(m.z * 6.2832);
                    float prog = f / 0.09;
                    float streak = 0.0;
                    [unroll]
                    for (int j = 0; j < 6; j++)
                    {
                        float pj = prog - j * 0.035;
                        float3 mp = normalize(s + tng * pj * 0.35);
                        streak += pow(saturate(dot(sd, mp)), 9000.0) * (1.0 - j / 6.0) * step(0.0, pj);
                    }
                    col += float3(0.9, 0.95, 1.0) * streak * (1.0 - prog * 0.6);
                }

                // Fade into the horizon haze and against a bright sky.
                float fade = smoothstep(0.0, 0.25, d.y);
                return col * _StarStrength * fade;
            }

            float3 moon(float3 d, float3 mdir)
            {
                float cosang = dot(d, mdir);
                float3 glow = _MoonColor.rgb * pow(saturate(cosang), 60.0) * _MoonGlow * 0.5;
                if (cosang < 0.9 || mdir.y < -0.05)
                    return glow;
                float3 upref = abs(mdir.y) > 0.98 ? float3(1, 0, 0) : float3(0, 1, 0);
                float3 mt = normalize(cross(mdir, upref));
                float3 mb = cross(mdir, mt);
                float2 uv = float2(dot(d, mt), dot(d, mb)) / _MoonSize;
                float r = length(uv);
                float disc = smoothstep(1.0, 0.93, r);
                float p = _MoonPhase;
                float side = p < 0.5 ? -1.0 : 1.0;
                float offx = side * min(p, 1.0 - p) * 4.2;
                float inShadow = 1.0 - smoothstep(0.95, 1.05, length(uv - float2(offx, 0)));
                float lit = 1.0 - inShadow * 0.94;                 // earthshine on the dark side
                float maria = 0.82 + 0.18 * vnoise(uv * 3.0 + 40.0);
                return glow + _MoonColor.rgb * disc * lit * maria;
            }

            fixed4 frag(v2f i) : SV_Target
            {
                float3 d = normalize(i.dir);
                float3 sunDir = normalize(_SunDir.xyz + float3(0, 1e-4, 0));
                float3 moonDir = normalize(_MoonDir.xyz + float3(0, 1e-4, 0));
                float h = d.y;

                // --- gradient ---
                float3 sky = lerp(_HorizonColor.rgb, _ZenithColor.rgb, pow(saturate(h), 0.55));
                float3 ground = lerp(_HorizonColor.rgb, _GroundColor.rgb, pow(saturate(-h), 0.35));
                float3 col = h >= 0.0 ? sky : ground;
                // Sunset glow: on the sun's side of the horizon, low in the sky.
                float2 dxz = normalize(d.xz + 1e-5);
                float2 sxz = normalize(sunDir.xz + 1e-5);
                float side = saturate(dot(dxz, sxz) * 0.5 + 0.5);
                float low = pow(saturate(1.0 - abs(h) * 2.2), 3.0);
                col += _HorizonGlowColor.rgb * _HorizonGlow * low * pow(side, 3.0) * 0.9;

                // Overcast: the whole dome goes flat grey before the clouds
                // are laid on, so a grey day reads grey between the clouds.
                float over = smoothstep(0.55, 0.95, _CloudCover);
                float lum = dot(col, float3(0.3, 0.5, 0.2));
                col = lerp(col, lum * float3(0.85, 0.9, 1.0) * 0.8, over);

                // --- stars + moon (behind the clouds) ---
                col += stars(d, sunDir);
                col += moon(d, moonDir);

                // --- sun ---
                float sdot = dot(d, sunDir);
                float disc = smoothstep(cos(_SunSize * 1.15), cos(_SunSize * 0.85), sdot);
                float glow = pow(saturate(sdot), 24.0) * _SunGlow
                           + pow(saturate(sdot), 3.0) * _SunGlow * 0.12;
                float aboveGround = smoothstep(-0.03, 0.0, h);
                col += _SunColor.rgb * (disc * 2.0 + glow) * aboveGround;

                // --- clouds ---
                if (h > 0.0)
                {
                    float2 uv = d.xz / (h + 0.2) * _CloudScale + _CloudOffset.xy
                              + float2(_Time.y, _Time.y * 0.35) * _CloudDrift;
                    float n = fbm(uv);
                    float thr = 1.0 - _CloudCover;
                    float dens = smoothstep(thr - 0.12, thr + 0.22, n);
                    float thick = smoothstep(thr, thr + 0.55, n);
                    float horizonFade = smoothstep(0.0, 0.16, h);
                    float alpha = dens * horizonFade;
                    float3 lit = _CloudColor.rgb;
                    float3 shade = _CloudShadeColor.rgb;
                    float3 cc = lerp(lit, shade, thick);
                    // Silver lining toward the sun, a cool rim toward the moon.
                    cc += _SunColor.rgb * pow(saturate(sdot), 6.0) * (1.0 - thick) * 0.6;
                    cc += _MoonColor.rgb * pow(saturate(dot(d, moonDir)), 8.0) * (1.0 - thick) * 0.25 * _MoonGlow;
                    cc *= lerp(1.0, 0.6, over);
                    col = lerp(col, cc, alpha);
                }

                return fixed4(col * _Exposure, 1.0);
            }
            ENDCG
        }
    }
    Fallback Off
}
