<script lang="ts">
  import '../app.css';
  import { onMount } from 'svelte';
  import { appState } from '$lib/stores/appState.svelte';

  let { children } = $props();

  onMount(() => {
    void appState.init();
  });
</script>

<svelte:window
  oncontextmenu={(e) => e.preventDefault()}
  onkeydown={(e) => {
    if (e.key === 'F5' || ((e.ctrlKey || e.metaKey) && (e.key === 'r' || e.key === 'R'))) {
      e.preventDefault();
    }
  }}
/>

{@render children?.()}
