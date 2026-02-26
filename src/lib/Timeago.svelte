<script lang="ts">
  import { format } from "timeago.js";

  interface Props {
    timestamp: number;
  }

  let { timestamp }: Props = $props();

  let timeago: string = $state("");
  let datetime = $derived(new Date(timestamp).toLocaleString());

  $effect(() => {
    // Re-read timestamp to track it as a dependency
    timeago = format(timestamp);

    function tick() {
      timeago = format(timestamp);
      const delta = Date.now() - timestamp;
      let delay: number;
      if (delta < 60 * 1000) {
        delay = 1000;
      } else if (delta < 60 * 60 * 1000) {
        delay = 60 * 1000;
      } else {
        delay = 60 * 60 * 1000;
      }
      timer = setTimeout(tick, delay);
    }

    let timer = setTimeout(tick, 1000);
    return () => clearTimeout(timer);
  });
</script>

<span title={datetime}>{timeago}</span>
