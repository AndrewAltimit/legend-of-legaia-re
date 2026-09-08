// Registers the villagers' baked navmesh (LegaiaNavMesh, the editor pass)
// with the runtime NavMesh when the world loads. There is no scene
// component for a NavMeshData asset without the AI Navigation package, so
// this one-liner is what turns the saved bake into something
// NavMesh.CalculatePath can query. Every client runs it locally; the
// navmesh is static data, nothing is synced.
//
// Requires UdonSharp (bundled with the VRChat worlds SDK).

using UdonSharp;
using UnityEngine;
using UnityEngine.AI;

namespace LegaiaWorld
{
    [UdonBehaviourSyncMode(BehaviourSyncMode.None)]
    public class LegaiaNavMeshLoader : UdonSharpBehaviour
    {
        [Tooltip("The baked NavMeshData asset (LegaiaGenerated/<scene>/livingtown/navmesh.asset).")]
        public NavMeshData data;

        [HideInInspector] public bool loaded;

        void Start()
        {
            if (data == null)
                return;
            NavMesh.AddNavMeshData(data);
            loaded = true;
        }
    }
}
