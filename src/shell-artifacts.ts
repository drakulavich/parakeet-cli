import type { ArgDef, ArgsDef, CommandDef, CommandMeta, Resolvable } from "citty";
import { completionsCommand } from "./cli/completions";
import { doctorCommand } from "./cli/doctor";
import { installCommand } from "./cli/install";
import { mainCommand } from "./cli/main";
import { manpageCommand } from "./cli/manpage";
import { sayCommand } from "./cli/say";
import { statsCommand } from "./cli/stats";
import { statusCommand } from "./cli/status";
import { supportBundleCommand } from "./cli/support-bundle";
import { packageVersion } from "./package-info";

type Shell = "bash" | "zsh" | "fish";

interface CliCommand {
  name: string;
  command: CommandDef<any>;
}

interface CliOption {
  name: string;
  description: string;
  takesValue: boolean;
}

interface ArtifactCommand {
  name: string;
  description: string;
  options: CliOption[];
  positional: string[];
}

interface ArtifactModel {
  root: ArtifactCommand;
  commands: ArtifactCommand[];
}

export interface ShellArtifact {
  path: string;
  content: string;
}

const CLI_COMMANDS: CliCommand[] = [
  { name: "completions", command: completionsCommand },
  { name: "doctor", command: doctorCommand },
  { name: "install", command: installCommand },
  { name: "manpage", command: manpageCommand },
  { name: "say", command: sayCommand },
  { name: "stats", command: statsCommand },
  { name: "status", command: statusCommand },
  { name: "support-bundle", command: supportBundleCommand },
];

const ROOT_BUILT_INS: CliOption[] = [
  { name: "--help", description: "Show help", takesValue: false },
  { name: "-h", description: "Show help", takesValue: false },
  { name: "--version", description: "Show version", takesValue: false },
  { name: "-v", description: "Show version", takesValue: false },
];

const HELP_OPTIONS: CliOption[] = [
  { name: "--help", description: "Show help", takesValue: false },
  { name: "-h", description: "Show help", takesValue: false },
];

async function resolve<T>(value: Resolvable<T> | undefined): Promise<T | undefined> {
  if (typeof value === "function") {
    return await (value as () => T | Promise<T>)();
  }
  return await value;
}

function sentence(value: string | undefined): string {
  return (value ?? "").replace(/\s+/g, " ").trim();
}

function optionFromArg(name: string, def: ArgDef): CliOption | null {
  if (def.type === "positional") return null;
  return {
    name: `--${name}`,
    description: sentence(def.description),
    takesValue: def.type === "string" || def.type === "enum",
  };
}

async function commandToArtifact(name: string, command: CommandDef<any>): Promise<ArtifactCommand> {
  const meta = (await resolve(command.meta)) as CommandMeta | undefined;
  const args = ((await resolve(command.args)) ?? {}) as ArgsDef;
  const options: CliOption[] = [];
  const positional: string[] = [];

  for (const [argName, def] of Object.entries(args)) {
    if (def.type === "positional") {
      positional.push(argName.toUpperCase());
      continue;
    }
    const option = optionFromArg(argName, def);
    if (option) options.push(option);
  }

  return {
    name,
    description: sentence(meta?.description),
    options,
    positional,
  };
}

async function buildModel(): Promise<ArtifactModel> {
  return {
    root: await commandToArtifact("kesha", mainCommand),
    commands: await Promise.all(CLI_COMMANDS.map((entry) => commandToArtifact(entry.name, entry.command))),
  };
}

function optionWords(options: CliOption[]): string {
  return options.map((option) => option.name).join(" ");
}

function renderBash(model: ArtifactModel): string {
  const rootOptions = optionWords([...ROOT_BUILT_INS, ...model.root.options]);
  const commands = model.commands.map((command) => command.name).join(" ");
  const cases = model.commands
    .map((command) => `    ${command.name}) opts="${optionWords([...HELP_OPTIONS, ...command.options])}" ;;`)
    .join("\n");

  return `# bash completion for kesha.
# Source this file or copy it into your bash-completion directory.

_kesha_completion() {
  local cur command opts commands
  COMPREPLY=()
  cur="\${COMP_WORDS[COMP_CWORD]}"
  command="\${COMP_WORDS[1]}"
  commands="${commands}"

  if [[ "$COMP_CWORD" -eq 1 ]]; then
    if [[ "$cur" == -* ]]; then
      COMPREPLY=( $(compgen -W "${rootOptions}" -- "$cur") )
    else
      COMPREPLY=( $(compgen -W "$commands" -- "$cur") )
    fi
    return 0
  fi

  case "$command" in
${cases}
    *) opts="${rootOptions}" ;;
  esac

  if [[ "$cur" == -* ]]; then
    COMPREPLY=( $(compgen -W "$opts" -- "$cur") )
  fi
}

complete -F _kesha_completion kesha
complete -F _kesha_completion parakeet
`;
}

function zshEscape(value: string): string {
  return value.replace(/\\/g, "\\\\").replace(/'/g, "'\\''").replace(/\[/g, "\\[").replace(/\]/g, "\\]");
}

function renderZshOption(option: CliOption): string {
  const desc = zshEscape(option.description);
  if (option.takesValue) {
    const valueName = option.name.slice(2).replace(/-/g, " ");
    return `'${option.name}=[${desc}]:${valueName}:'`;
  }
  return `'${option.name}[${desc}]'`;
}

function renderZsh(model: ArtifactModel): string {
  const commands = model.commands
    .map((command) => `    '${command.name}:${zshEscape(command.description)}'`)
    .join("\n");
  const rootOptions = [...ROOT_BUILT_INS, ...model.root.options].map(renderZshOption).join(" \\\n      ");
  const cases = model.commands
    .map((command) => {
      const options = [...HELP_OPTIONS, ...command.options].map(renderZshOption).join(" \\\n        ");
      return `    ${command.name})\n      _arguments ${options}\n      ;;`;
    })
    .join("\n");

  return `#compdef kesha parakeet
# zsh completion for kesha.

_kesha() {
  local -a commands
  commands=(
${commands}
  )

  if (( CURRENT == 2 )); then
    if [[ "$words[CURRENT]" == -* ]]; then
      _arguments ${rootOptions}
    else
      _describe -t commands 'kesha command' commands
    fi
    return
  fi

  case "$words[2]" in
${cases}
    *)
      _arguments ${rootOptions}
      ;;
  esac
}

_kesha "$@"
`;
}

function fishEscape(value: string): string {
  return value.replace(/\\/g, "\\\\").replace(/'/g, "\\'");
}

function renderFishOption(bin: string, condition: string, option: CliOption): string {
  const long = option.name.startsWith("--") ? ` -l ${option.name.slice(2)}` : "";
  const short = option.name.startsWith("-") && !option.name.startsWith("--") ? ` -s ${option.name.slice(1)}` : "";
  const required = option.takesValue ? " -r" : "";
  return `complete -c ${bin}${condition}${long}${short}${required} -d '${fishEscape(option.description)}'`;
}

function renderFish(model: ArtifactModel): string {
  const bins = ["kesha", "parakeet"];
  const lines = ["# fish completion for kesha."];

  for (const bin of bins) {
    lines.push(`complete -c ${bin} -f`);
    for (const command of model.commands) {
      lines.push(
        `complete -c ${bin} -n '__fish_use_subcommand' -a '${command.name}' -d '${fishEscape(command.description)}'`,
      );
    }
    for (const option of [...ROOT_BUILT_INS, ...model.root.options]) {
      lines.push(renderFishOption(bin, " -n '__fish_use_subcommand'", option));
    }
    for (const command of model.commands) {
      const condition = ` -n '__fish_seen_subcommand_from ${command.name}'`;
      for (const option of [...HELP_OPTIONS, ...command.options]) {
        lines.push(renderFishOption(bin, condition, option));
      }
    }
  }

  return `${lines.join("\n")}\n`;
}

function roff(value: string): string {
  return value.replace(/\\/g, "\\\\").replace(/-/g, "\\-");
}

function manpageDate(): string {
  const now = new Date();
  const month = now.toLocaleString("en-US", { month: "long" });
  return `${month} ${now.getFullYear()}`;
}

function renderManpage(model: ArtifactModel): string {
  const commandSections = model.commands
    .map((command) => `.TP\n.B ${command.name}\n${roff(command.description)}`)
    .join("\n");
  const rootOptions = model.root.options
    .map((option) => `.TP\n.B ${roff(option.name)}\n${roff(option.description)}`)
    .join("\n");
  const subcommandOptions = model.commands
    .filter((command) => command.options.length > 0)
    .map((command) => {
      const options = command.options
        .map((option) => `.TP\n.B ${roff(option.name)}\n${roff(option.description)}`)
        .join("\n");
      return `.SS ${command.name}\n${options}`;
    })
    .join("\n");

  return `.TH KESHA 1 "${manpageDate()}" "kesha ${packageVersion}" "User Commands"
.SH NAME
kesha \\- open-source local voice toolkit
.SH SYNOPSIS
.B kesha
[OPTIONS] AUDIO_FILE...
.br
.B kesha
COMMAND [OPTIONS]
.SH DESCRIPTION
Kesha Voice Kit is a local speech-to-text and text-to-speech toolkit with a Bun TypeScript CLI and a Rust engine.
.SH COMMANDS
${commandSections}
.SH OPTIONS
${rootOptions}
.SH SUBCOMMAND OPTIONS
${subcommandOptions}
.SH FILES
.TP
.B completions/kesha.bash
Bash completion script included in the npm package.
.TP
.B completions/kesha.zsh
Zsh completion script included in the npm package.
.TP
.B completions/kesha.fish
Fish completion script included in the npm package.
.SH SEE ALSO
.BR bun (1)
`;
}

export async function generateShellArtifacts(): Promise<ShellArtifact[]> {
  const model = await buildModel();
  const renderers: Record<Shell, (model: ArtifactModel) => string> = {
    bash: renderBash,
    zsh: renderZsh,
    fish: renderFish,
  };

  return [
    ...Object.entries(renderers).map(([shell, render]) => ({
      path: `completions/kesha.${shell}`,
      content: render(model),
    })),
    { path: "man/kesha.1", content: renderManpage(model) },
  ];
}
