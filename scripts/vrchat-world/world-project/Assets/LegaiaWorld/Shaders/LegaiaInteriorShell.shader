// The interior-room "mini skydome": a plain unlit colour (black) drawn on
// the FRONT face only. The realism pass winds each shell dome so its front
// faces point inward in world space (flipping for the built root's mirror),
// which makes the shell:
//   - solid black from inside a room (the sky above and the doorway behind
//     read as black space, retail's own framing for these rooms), and
//   - invisible from outside (backface-culled), so the village view and
//     the room exteriors are untouched.
// The shell renderer also casts no shadows, so the sun still lights the
// room through it - the "Legaia/" name prefix keeps the lit-conversion
// pass from touching this material.
Shader "Legaia/Interior Shell"
{
    Properties
    {
        _Color ("Color", Color) = (0,0,0,1)
    }
    SubShader
    {
        Tags { "RenderType"="Opaque" "Queue"="Geometry" }
        Cull Back
        LOD 100

        Pass
        {
            CGPROGRAM
            #pragma vertex vert
            #pragma fragment frag
            #include "UnityCG.cginc"

            fixed4 _Color;

            float4 vert (float4 vertex : POSITION) : SV_POSITION
            {
                return UnityObjectToClipPos(vertex);
            }

            fixed4 frag () : SV_Target
            {
                return _Color;
            }
            ENDCG
        }
    }
}
