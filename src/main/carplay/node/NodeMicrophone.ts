import { spawn, ChildProcessWithoutNullStreams, execSync } from 'child_process';
import { EventEmitter } from 'events';
import os from 'os';

export default class NodeMicrophone extends EventEmitter {
  private process: ChildProcessWithoutNullStreams | null = null;
  private readonly device: string;
  private readonly rate: number = 16000;
  private readonly channels: number = 1;
  private readonly format: string = 'S16_LE';

  constructor() {
    super();
    this.device = NodeMicrophone.resolveSysdefaultDevice();
    console.debug('[NodeMicrophone] Using device:', this.device);
  }

  start(): void {
    this.stop();

    let cmd = '';
    let args: string[] = [];

    if (os.platform() === 'linux') {
      cmd = 'arecord';
      args = [
        '-D', this.device,
        '-f', this.format,
        '-c', this.channels.toString(),
        '-r', this.rate.toString(),
        '-t', 'raw',
        '-q',
        '-'
      ];
    } else if (os.platform() === 'darwin') {
      // Requires: brew install sox
      cmd = 'rec'; 
      args = [
        '-b', '16',
        '-c', this.channels.toString(),
        '-r', this.rate.toString(),
        '-e', 'signed-integer',
        '-t', 'raw',
        '-q',
        '-'
      ];
    } else {
      console.error('[NodeMicrophone] Platform not supported for microphone recording');
      return;
    }

    console.debug(`[NodeMicrophone] Spawning ${cmd} with args:`, args.join(' '));
    this.process = spawn(cmd, args);

    this.process.stdout.on('data', (chunk: Buffer) => {
      this.emit('data', chunk);
    });

    this.process.stderr.on('data', (data: Buffer) => {
      console.warn('[NodeMicrophone] STDERR:', data.toString().trim());
    });

    this.process.on('error', err => {
      console.error('[NodeMicrophone] Error:', err);
      this.cleanup();
    });

    this.process.on('close', code => {
      console.debug(`[NodeMicrophone] ${cmd} exited with code`, code);
      this.cleanup();
    });

    console.debug('[NodeMicrophone] Recording started');
  }

  stop(): void {
    if (this.process) {
      console.debug('[NodeMicrophone] Stopping recording');
      try {
        this.process.kill();
      } catch (e) {
        console.warn('[NodeMicrophone] Failed to kill process:', e);
      }
      this.cleanup();
    } else {
      console.debug('[NodeMicrophone] No active process to stop');
    }
  }

  private cleanup(): void {
    this.process = null;
  }

  static resolveSysdefaultDevice(): string {
    if (os.platform() === 'linux') {
      try {
        const output = execSync('arecord -L', { encoding: 'utf8' });
        const lines = output.split('\n');
        for (const line of lines) {
          const trimmed = line.trim();
          const match = trimmed.match(/^sysdefault:CARD=([^\s,]+)/);
          if (match) {
            const card = match[1];
            return `plughw:CARD=${card},DEV=0`;
          }
        }
        console.warn('[NodeMicrophone] sysdefault card not found, falling back');
        return 'plughw:0,0';
      } catch (e) {
        console.warn('[NodeMicrophone] Failed to resolve sysdefault device', e);
        return 'plughw:0,0';
      }
    } else if (os.platform() === 'darwin') {
      return 'default';
    } else {
      return 'unsupported';
    }
  }

  static getSysdefaultPrettyName(): string {
    if (os.platform() === 'linux') {
      try {
        const result = execSync('arecord -L', { encoding: 'utf8' });
        const lines = result.split('\n');
        const index = lines.findIndex(line => line.trim().startsWith('sysdefault:'));
        if (index === -1) return 'not available';

        const description = lines[index + 1]?.trim();
        if (!description || description === 'sysdefault') return 'not available';

        return description;
      } catch (e) {
        console.warn('[NodeMicrophone] Failed to get sysdefault mic label', e);
        return 'not available';
      }
    } else if (os.platform() === 'darwin') {
      return 'system default';
    } else {
      return 'not available';
    }
  }
}
