import type { EditorBase, EditCell, ColumnRegular } from "@revolist/revogrid";

export class TextAreaEditor implements EditorBase {
	public element: HTMLTextAreaElement | null = null;
	public editCell: EditCell | undefined;
	private container: HTMLDivElement | null = null;

	constructor(
		public column: ColumnRegular,
		private saveCallback: (value: string) => void,
	) {}

	componentDidRender() {}

	disconnectedCallback() {
		this.destroy();
	}

	private destroy() {
		if (this.container && this.container.parentNode) {
			this.container.parentNode.removeChild(this.container);
		}
		this.container = null;
		this.element = null;
	}

	render(createElement: any) {
		// Create a container div that will be positioned
		this.container = document.createElement("div");
		this.container.className = "textarea-editor-container";
		this.container.style.cssText = `
			position: fixed;
			z-index: 9999;
			background: var(--background, #fff);
			border: 1px solid var(--border, #e2e8f0);
			border-radius: 8px;
			box-shadow: 0 10px 25px -5px rgba(0, 0, 0, 0.1), 0 8px 10px -6px rgba(0, 0, 0, 0.1);
			padding: 8px;
			display: flex;
			flex-direction: column;
			gap: 8px;
		`;

		// Create textarea
		this.element = document.createElement("textarea");
		this.element.className = "textarea-editor";
		this.element.style.cssText = `
			min-width: 250px;
			min-height: 120px;
			max-width: 50vw;
			max-height: 35vh;
			width: 350px;
			height: 150px;
			resize: both;
			overflow: auto;
			padding: 10px 12px;
			font-family: inherit;
			font-size: 14px;
			line-height: 1.5;
			border: 1px solid var(--border, #e2e8f0);
			border-radius: 6px;
			background: var(--background, #fff);
			color: var(--foreground, #0f172a);
			outline: none;
			transition: border-color 0.15s ease;
		`;

		// Add focus styles
		this.element.addEventListener("focus", () => {
			if (this.element) {
				this.element.style.borderColor = "var(--ring, #3b82f6)";
				this.element.style.boxShadow = "0 0 0 2px rgba(59, 130, 246, 0.2)";
			}
		});

		this.element.addEventListener("blur", () => {
			if (this.element) {
				this.element.style.borderColor = "var(--border, #e2e8f0)";
				this.element.style.boxShadow = "none";
			}
		});

		// Create button container
		const buttonContainer = document.createElement("div");
		buttonContainer.style.cssText = `
			display: flex;
			justify-content: flex-end;
			gap: 8px;
		`;

		// Cancel button
		const cancelBtn = document.createElement("button");
		cancelBtn.textContent = "Cancel";
		cancelBtn.type = "button";
		cancelBtn.style.cssText = `
			padding: 6px 14px;
			font-size: 13px;
			font-weight: 500;
			border: 1px solid var(--border, #e2e8f0);
			border-radius: 6px;
			background: var(--background, #fff);
			color: var(--foreground, #0f172a);
			cursor: pointer;
			transition: background-color 0.15s ease;
		`;
		cancelBtn.addEventListener("mouseenter", () => {
			cancelBtn.style.backgroundColor = "var(--muted, #f1f5f9)";
		});
		cancelBtn.addEventListener("mouseleave", () => {
			cancelBtn.style.backgroundColor = "var(--background, #fff)";
		});
		cancelBtn.addEventListener("click", (e) => {
			e.preventDefault();
			e.stopPropagation();
			this.destroy();
		});

		// Save button
		const saveBtn = document.createElement("button");
		saveBtn.textContent = "Save";
		saveBtn.type = "button";
		saveBtn.style.cssText = `
			padding: 6px 14px;
			font-size: 13px;
			font-weight: 500;
			border: 1px solid transparent;
			border-radius: 6px;
			background: var(--primary, #3b82f6);
			color: var(--primary-foreground, #fff);
			cursor: pointer;
			transition: opacity 0.15s ease;
		`;
		saveBtn.addEventListener("mouseenter", () => {
			saveBtn.style.opacity = "0.9";
		});
		saveBtn.addEventListener("mouseleave", () => {
			saveBtn.style.opacity = "1";
		});
		saveBtn.addEventListener("click", (e) => {
			e.preventDefault();
			e.stopPropagation();
			if (this.element) {
				this.saveCallback(this.element.value);
			}
			this.destroy();
		});

		buttonContainer.appendChild(cancelBtn);
		buttonContainer.appendChild(saveBtn);

		this.container.appendChild(this.element);
		this.container.appendChild(buttonContainer);

		// Position the container near the cell
		document.body.appendChild(this.container);

		// Handle Escape key to cancel
		this.element.addEventListener("keydown", (e) => {
			if (e.key === "Escape") {
				e.preventDefault();
				e.stopPropagation();
				this.destroy();
			}
			// Ctrl/Cmd + Enter to save
			if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
				e.preventDefault();
				e.stopPropagation();
				if (this.element) {
					this.saveCallback(this.element.value);
				}
				this.destroy();
			}
			// Stop propagation for regular Enter to allow newlines
			if (e.key === "Enter" && !e.ctrlKey && !e.metaKey) {
				e.stopPropagation();
			}
		});

		// Return a placeholder element for RevoGrid
		return createElement("span", { style: { display: "none" } });
	}

	setValue(value: any) {
		if (this.element) {
			this.element.value = value ?? "";

			// Position the popup near the cell
			requestAnimationFrame(() => {
				if (this.editCell && this.container) {
					const cellRect = {
						top: this.editCell.y ?? 100,
						left: this.editCell.x ?? 100,
					};

					// Calculate position ensuring it stays within viewport
					let top = cellRect.top;
					let left = cellRect.left;

					const containerRect = this.container.getBoundingClientRect();
					const viewportWidth = window.innerWidth;
					const viewportHeight = window.innerHeight;

					// Adjust if would go off right edge
					if (left + containerRect.width > viewportWidth - 20) {
						left = viewportWidth - containerRect.width - 20;
					}

					// Adjust if would go off bottom edge
					if (top + containerRect.height > viewportHeight - 20) {
						top = viewportHeight - containerRect.height - 20;
					}

					// Ensure minimum positions
					left = Math.max(20, left);
					top = Math.max(20, top);

					this.container.style.top = `${top}px`;
					this.container.style.left = `${left}px`;
				}

				// Focus the textarea
				this.element?.focus();
				this.element?.select();
			});
		}
	}

	getValue() {
		return this.element?.value ?? "";
	}
}

export function createTextAreaEditor(saveCallback: (value: string, cell: EditCell) => void) {
	return (column: ColumnRegular, saveHandler: (value: string) => void, cancelHandler: () => void) => {
		return new TextAreaEditor(column, saveHandler);
	};
}
