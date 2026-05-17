<script lang="ts">
	import { BookOpen, Clock, FileText } from "@lucide/svelte";
</script>

<div class="p-8 max-w-2xl mx-auto">
	<div class="text-center space-y-6">
		<!-- Icon -->
		<div class="flex justify-center">
			<div class="relative">
				<BookOpen class="h-24 w-24 text-primary/20" />
				<Clock
					class="h-8 w-8 text-primary absolute bottom-0 right-0 bg-background rounded-full p-1"
				/>
			</div>
		</div>

		<!-- Main Content -->
		<div class="space-y-4">
			<h1 class="text-4xl font-bold text-foreground">Documentation</h1>
			<p class="text-xl text-muted-foreground max-w-md mx-auto">
				Comprehensive guides and reference materials for metadata management
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
					<FileText class="h-5 w-5 text-primary mt-0.5 flex-shrink-0" />
					<span>Step-by-step workflow guides and tutorials</span>
				</li>
				<li class="flex items-start gap-2">
					<FileText class="h-5 w-5 text-primary mt-0.5 flex-shrink-0" />
					<span>Best practices for metadata quality and consistency</span>
				</li>
				<li class="flex items-start gap-2">
					<FileText class="h-5 w-5 text-primary mt-0.5 flex-shrink-0" />
					<span>API documentation and integration examples</span>
				</li>
				<li class="flex items-start gap-2">
					<FileText class="h-5 w-5 text-primary mt-0.5 flex-shrink-0" />
					<span>Troubleshooting guides and FAQ section</span>
				</li>
				<li class="flex items-start gap-2">
					<FileText class="h-5 w-5 text-primary mt-0.5 flex-shrink-0" />
					<span>Video tutorials and interactive demos</span>
				</li>
			</ul>
		</div>

		<!-- Quick Links -->
		<div class="bg-muted/50 border border-border rounded-lg p-4">
			<h4 class="text-sm font-medium text-foreground mb-2">Meanwhile, try these resources:</h4>
			<div class="space-y-1 text-sm text-muted-foreground">
				<p>• Use the <strong>Assist</strong> tab to start metadata collection</p>
				<p>• Check project settings in the sidebar</p>
				<p>• Export your data from the main dashboard</p>
			</div>
		</div>

		<!-- Progress Indicator -->
		<div class="space-y-2">
			<div class="flex justify-between text-sm text-muted-foreground">
				<span>Development Progress</span>
				<span>Documentation Phase</span>
			</div>
			<div class="w-full bg-muted rounded-full h-2">
				<div class="bg-primary h-2 rounded-full w-1/8"></div>
			</div>
		</div>
	</div>
</div>
