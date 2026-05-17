<main>
	<article>
		<section>
			<h1><span class="dt">q</span><u>rate</u></h1>
			<h2>
				<span>Your reliable workspace</span><br />
				for <b>streamlining</b> digital asset indexing
			</h2>
			<p>
				<span class="lg"><span class="dt">q</span><u>rate</u></span> is the quintessential workspace
				for archivists, curators, librarians, and everyone else who finds themselves often indexing
				vital information.
				<br /><br />
				Powered by state-of-the-art <span class="lg">cohere</span> multimodal MCP tool-calling vision
				AI models, we feature stunning accuracy and shockingly efficient processing, ready to save you
				months of time.
			</p>
			<div class="button-group">
				<a href="./new" class="cta">Go to Projects</a>
				<a
					href="https://archivescanada.ca/wp-content/uploads/2022/08/RADComplete_July2008.pdf"
					target="_blank"
					class="secondary">Canadian Archival Standard</a
				>
			</div>
		</section>
	</article>
</main>

<style>
	.dt {
		position: relative;
		&::before {
			content: "";
			position: absolute;
			bottom: 0.09em;
			left: 0.07em;
			border-radius: 50%;
			background-color: var(--dark);
			width: 0.17em;
			height: 0.17em;
		}
	}
	u {
		text-decoration-thickness: 0.17em;
	}
	.lg {
		font-weight: 800;
	}
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
		min-height: 100vh;
		background: var(--dark);
		padding: 2rem;
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

	@keyframes main {
		0% {
			rotate: -13deg;
		}
		50% {
			rotate: 13deg;
		}
		100% {
			rotate: -13deg;
		}
	}

	article {
		background: var(--white);
		padding: 3rem 4rem;
		border-radius: var(--radius);
		box-shadow: 0 8px 24px rgba(0, 0, 0, 0.08);
		text-align: center;
		width: calc(100vw - 14em);
		border-top: 6px solid var(--primary);
	}

	/* Logo */
	.logo {
		width: 4em;
		margin-bottom: -2em;
		margin-top: -1.4em;
		display: block;
		margin-left: auto;
		margin-right: auto;
	}

	/* Typography */
	h1 {
		font-size: 5.2rem;
		font-weight: 800;
		margin: 0.2em 0;
		color: var(--dark);
	}

	h2 {
		font-size: 1.5rem;
		font-weight: 400;
		margin: 0.5em 0 1.5em;
		line-height: 1.4;
		color: var(--dark);
	}

	p {
		line-height: 1.6;
		margin: 2em auto;
		font-size: 1.2rem;
		max-width: 32em;
		color: #444;
		text-align: center;
	}

	/* Buttons */
	.button-group {
		display: flex;
		justify-content: center;
		gap: 1em;
		margin-top: 2em;
	}

	.cta,
	.secondary {
		display: inline-block;
		padding: 0.9em 1.6em;
		font-size: 1rem;
		font-weight: 600;
		text-decoration: none;
		border-radius: var(--radius);
		transition:
			background var(--transition),
			transform var(--transition),
			box-shadow var(--transition);
	}

	/* Primary button */
	.cta {
		background: var(--primary);
		color: var(--white);
		box-shadow: 0 4px 12px rgba(139, 28, 36, 0.3);
	}
	.cta:hover {
		background: #6e151b;
		transform: translateY(-2px);
		box-shadow: 0 6px 16px rgba(139, 28, 36, 0.4);
	}

	/* Secondary button */
	.secondary {
		background: var(--background);
		color: var(--primary);
		border: 2px solid var(--primary);
	}
	.secondary:hover {
		background: var(--primary);
		color: var(--white);
		transform: translateY(-2px);
	}
</style>
