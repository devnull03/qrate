<script lang="ts">
	import { CheckCircle2, Clock } from "@lucide/svelte";
</script>

<div class="p-8 max-w-2xl mx-auto">
	<div class="text-center space-y-6">
		<!-- Icon -->
		<div class="flex justify-center">
			<div class="relative">
				<CheckCircle2 class="h-24 w-24 text-primary/20" />
				<Clock
					class="h-8 w-8 text-primary absolute bottom-0 right-0 bg-background rounded-full p-1"
				/>
			</div>
		</div>

		<!-- Main Content -->
		<div class="space-y-4">
			<h1 class="text-4xl font-bold text-foreground">Review & Validate</h1>
			<p class="text-xl text-muted-foreground max-w-md mx-auto">
				Quality control and verification tools for your metadata assignments
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
					<CheckCircle2 class="h-5 w-5 text-primary mt-0.5 flex-shrink-0" />
					<span>Review and validate AI-generated metadata</span>
				</li>
				<li class="flex items-start gap-2">
					<CheckCircle2 class="h-5 w-5 text-primary mt-0.5 flex-shrink-0" />
					<span>Batch approval and rejection workflows</span>
				</li>
				<li class="flex items-start gap-2">
					<CheckCircle2 class="h-5 w-5 text-primary mt-0.5 flex-shrink-0" />
					<span>Quality scoring and confidence metrics</span>
				</li>
				<li class="flex items-start gap-2">
					<CheckCircle2 class="h-5 w-5 text-primary mt-0.5 flex-shrink-0" />
					<span>Export reviewed data in multiple formats</span>
				</li>
			</ul>
		</div>

		<!-- Progress Indicator -->
		<div class="space-y-2">
			<div class="flex justify-between text-sm text-muted-foreground">
				<span>Development Progress</span>
				<span>Coming Next Sprint</span>
			</div>
			<div class="w-full bg-muted rounded-full h-2">
				<div class="bg-primary h-2 rounded-full w-1/4"></div>
			</div>
		</div>
	</div>
</div>
