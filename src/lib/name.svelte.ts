import { shortId, getDisplayName } from "$lib/utils";

/**
 * Reactive display name resolution using Svelte 5 runes.
 * Returns an object with a reactive `name` property that starts with a
 * synchronous fallback and updates when the async profile lookup completes.
 *
 * Must be called from a component's top-level script (so $effect binds correctly).
 */
export function useDisplayName(
  getPubkey: () => string,
  getSelfId: () => string,
): { readonly name: string } {
  let name = $state("");

  $effect(() => {
    const pubkey = getPubkey();
    const selfId = getSelfId();
    if (!pubkey) {
      name = "";
      return;
    }
    name = pubkey === selfId ? "You" : shortId(pubkey);
    getDisplayName(pubkey, selfId).then((resolved) => {
      name = resolved;
    });
  });

  return {
    get name() {
      return name;
    },
  };
}
