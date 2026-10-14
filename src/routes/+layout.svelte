<script lang="ts">
  import '../app.css';
  import { onMount } from 'svelte';
  import { appState } from '$lib/stores/appState.svelte';

  let { children } = $props();

  onMount(() => {
    void appState.init();
  });
</script>

{@render children?.()}
