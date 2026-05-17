<script lang="ts">
	import { BarChart3, Clock, TrendingUp } from "@lucide/svelte";
</script>

<div class="p-8 max-w-2xl mx-auto">
	<div class="text-center space-y-6">
		<!-- Icon -->
		<div class="flex justify-center">
			<div class="relative">
				<BarChart3 class="h-24 w-24 text-primary/20" />
				<Clock
					class="h-8 w-8 text-primary absolute bottom-0 right-0 bg-background rounded-full p-1"
				/>
			</div>
		</div>

		<!-- Main Content -->
		<div class="space-y-4">
			<h1 class="text-4xl font-bold text-foreground">Data Visualization</h1>
			<p class="text-xl text-muted-foreground max-w-md mx-auto">
				Interactive charts and insights for your metadata collection progress
			</p>
		</div>

		<!-- Coming Soon Badge -->
		<div class="inline-flex items-center gap-2 bg-primary/10 text-primary px-4 py-2 rounded-full">
			<Clock class="h-4 w-4" />
			<span class="font-medium">Coming Soon</span>
		</div>

		<!-- Feature Preview -->
		<div class="bg-card border border-border rounded-lg p-6 text-left space-y-4">
			<h3 class="text-lg font-semibold text-foreground">What's Coming</h3>
			<ul class="space-y-2 text-muted-foreground">
				<li class="flex items-start gap-2">
					<TrendingUp class="h-5 w-5 text-primary mt-0.5 flex-shrink-0" />
					<span>Progress tracking and completion analytics</span>
				</li>
				<li class="flex items-start gap-2">
					<TrendingUp class="h-5 w-5 text-primary mt-0.5 flex-shrink-0" />
					<span>Interactive charts and data distribution views</span>
				</li>
				<li class="flex items-start gap-2">
					<TrendingUp class="h-5 w-5 text-primary mt-0.5 flex-shrink-0" />
					<span>Metadata quality and completeness metrics</span>
				</li>
				<li class="flex items-start gap-2">
					<TrendingUp class="h-5 w-5 text-primary mt-0.5 flex-shrink-0" />
					<span>Time-based workflow insights and trends</span>
				</li>
				<li class="flex items-start gap-2">
					<TrendingUp class="h-5 w-5 text-primary mt-0.5 flex-shrink-0" />
					<span>Exportable reports and data summaries</span>
				</li>
			</ul>
		</div>

		<!-- Progress Indicator -->
		<div class="space-y-2">
			<div class="flex justify-between text-sm text-muted-foreground">
				<span>Development Progress</span>
				<span>In Planning Phase</span>
			</div>
			<div class="w-full bg-muted rounded-full h-2">
				<div class="bg-primary h-2 rounded-full w-1/6"></div>
			</div>
		</div>
	</div>
</div>
