/**
 * Command Registry Service
 *
 * A simple command registry for registering and executing commands.
 * Commands can be triggered by status bar items, keyboard shortcuts, or other UI elements.
 */

export type CommandHandler = (...args: unknown[]) => void | Promise<void>;

export interface ICommandRegistry {
	/**
	 * Register a command handler.
	 * @param id - Unique command identifier
	 * @param handler - Function to execute when the command is invoked
	 * @returns Dispose function to unregister the command
	 */
	register(id: string, handler: CommandHandler): () => void;

	/**
	 * Execute a registered command.
	 * @param id - Command identifier
	 * @param args - Arguments to pass to the command handler
	 * @throws Error if the command is not registered
	 */
	execute(id: string, ...args: unknown[]): Promise<void>;

	/**
	 * Check if a command is registered.
	 * @param id - Command identifier
	 */
	has(id: string): boolean;

	/**
	 * Get all registered command IDs.
	 */
	getCommands(): string[];
}

class CommandRegistry implements ICommandRegistry {
	private commands = new Map<string, CommandHandler>();
	private listeners = new Set<(commandId: string, type: "registered" | "unregistered") => void>();

	register(id: string, handler: CommandHandler): () => void {
		if (this.commands.has(id)) {
			console.warn(`[CommandRegistry] Command "${id}" is already registered. Overwriting.`);
		}

		this.commands.set(id, handler);
		this.notifyListeners(id, "registered");

		// Return dispose function
		return () => {
			if (this.commands.get(id) === handler) {
				this.commands.delete(id);
				this.notifyListeners(id, "unregistered");
			}
		};
	}

	async execute(id: string, ...args: unknown[]): Promise<void> {
		const handler = this.commands.get(id);

		if (!handler) {
			console.error(`[CommandRegistry] Command "${id}" is not registered.`);
			throw new Error(`Command "${id}" is not registered.`);
		}

		try {
			await handler(...args);
		} catch (error) {
			console.error(`[CommandRegistry] Error executing command "${id}":`, error);
			throw error;
		}
	}

	has(id: string): boolean {
		return this.commands.has(id);
	}

	getCommands(): string[] {
		return Array.from(this.commands.keys());
	}

	/**
	 * Subscribe to command registration changes.
	 * @param listener - Callback for registration events
	 * @returns Unsubscribe function
	 */
	onDidChangeCommands(
		listener: (commandId: string, type: "registered" | "unregistered") => void
	): () => void {
		this.listeners.add(listener);
		return () => {
			this.listeners.delete(listener);
		};
	}

	private notifyListeners(commandId: string, type: "registered" | "unregistered"): void {
		for (const listener of this.listeners) {
			try {
				listener(commandId, type);
			} catch (error) {
				console.error("[CommandRegistry] Error in listener:", error);
			}
		}
	}

	/**
	 * Clear all registered commands (useful for testing).
	 */
	clear(): void {
		const commandIds = Array.from(this.commands.keys());
		this.commands.clear();
		for (const id of commandIds) {
			this.notifyListeners(id, "unregistered");
		}
	}
}

// Export singleton instance
export const commandRegistry = new CommandRegistry();

// Export type for testing or custom instances
export type { CommandRegistry };
