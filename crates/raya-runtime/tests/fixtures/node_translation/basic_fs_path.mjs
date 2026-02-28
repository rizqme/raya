import fs from 'node:fs';
import path from 'node:path';

export function touch(file) {
  return { fs, path, file };
}
