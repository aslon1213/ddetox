<script lang="ts">
  import WeekCalendar from "$lib/components/WeekCalendar.svelte";
</script>

<h1>Calendar</h1>
<p class="lede">
  When each session blocks, across the week. All-day rules fill the column; timed
  windows show as blocks. Navigate weeks to see date-range and every-N-day rules.
</p>

<WeekCalendar />

<style>
  .lede {
    margin: 0;
    color: #9a9a9e;
    line-height: 1.5;
  }
</style>
