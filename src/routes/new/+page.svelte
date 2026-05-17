<script lang="ts">
	import ProjectButtons from "./projectbuttons.svelte";
</script>

<main>
	<article>
		<h1>Select Project</h1>
		<svelte:boundary>
			{#snippet pending()}
				<p>Loading...</p>
			{/snippet}
			<ProjectButtons />
		</svelte:boundary>
	</article>
</main>

<style>
	:root {
		--primary: var(--dark);
		--secondary: var(--dark);
		--background: var(--light);
		--text: #2b2b2b;
		--white: #ffffff;
		--radius: 12px;
		--transition: 0.25s ease-in-out;
	}

	/* Layout */
	main {
		display: flex;
		justify-content: center;
		align-items: center;
		height: 100vh;
		width: 100%;
		margin: 0;
		padding: 2rem;
		background-color: var(--dark);
		font-family: system-ui, sans-serif;
		color: var(--text);
		--s: 193px; /* control the size*/
		--c1: #921c2d;
		--c2: #880d1e;
		--c3: #5b0f1a;

		--_g: var(--c1) 10%, var(--c2) 10.5% 19%, #0000 19.5% 80.5%, var(--c2) 81% 89.5%, var(--c3) 90%;
		--_c: from -90deg at 37.5% 50%, #0000 75%;
		--_l1: linear-gradient(145deg, var(--_g));
		--_l2: linear-gradient(35deg, var(--_g));
		background:
			var(--_l1),
			var(--_l1) calc(var(--s) / 2) var(--s),
			var(--_l2),
			var(--_l2) calc(var(--s) / 2) var(--s),
			conic-gradient(var(--_c), var(--c1) 0) calc(var(--s) / 8) 0,
			conic-gradient(var(--_c), var(--c3) 0) calc(var(--s) / 2) 0,
			linear-gradient(90deg, var(--c3) 38%, var(--c1) 0 50%, var(--c3) 0 62%, var(--c1) 0);
		background-size: var(--s) calc(2 * var(--s) / 3);
	}

	/* Card */
	article {
		background: var(--white);
		padding: 3rem 4rem;
		border-radius: var(--radius);
		box-shadow: 0 8px 24px rgba(0, 0, 0, 0.08);
		text-align: center;
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 2em;
		border-top: 6px solid var(--primary);
		max-width: 600px;
		width: 100%;
	}

	/* Title */
	h1 {
		font-size: 2.5rem;
		font-weight: 800;
		margin: 0;
		color: var(--primary);
	}

	/* Loading state */
	p {
		font-size: 1.2rem;
		color: var(--secondary);
	}
</style>
