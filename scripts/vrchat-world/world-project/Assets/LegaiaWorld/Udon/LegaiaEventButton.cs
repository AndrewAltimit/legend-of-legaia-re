// A world button: Interact on this object's collider sends one named
// custom event into a target behaviour. The kit's common prefabs (mirror
// quality buttons, the TV's transport buttons, the card table's shuffle /
// gather) all use this instead of a world-space UI canvas - a collider
// button needs no VRCUiShape, so it cannot fall into the pointer trap
// where a nearby pickup collider steals the click (see LegaiaCampProps'
// settings-panel note).
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaEventButton : UdonSharpBehaviour
    {
        [Tooltip("Behaviour that receives the event.")]
        public UdonSharpBehaviour target;

        [Tooltip("Public method name on the target (SendCustomEvent).")]
        public string eventName;

        [Tooltip("Prompt shown when a player looks at the button.")]
        public string interactText = "Use";

        void Start()
        {
            if (!string.IsNullOrEmpty(interactText))
                InteractionText = interactText;
        }

        public override void Interact()
        {
            if (target != null && !string.IsNullOrEmpty(eventName))
                target.SendCustomEvent(eventName);
        }
    }
}
