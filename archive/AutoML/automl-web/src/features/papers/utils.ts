import type { Author, Paper, PaperPackage } from './types';

export const normalizeAuthors = (authors: Paper['authors'] | PaperPackage['authors']): Author[] => {
  if (!authors) return [];
  if (Array.isArray(authors)) return authors;
  try {
    const parsed = JSON.parse(authors);
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
};

export const authorLabel = (author: Author) =>
  [author.name, author.openclaw_handle].filter(Boolean).join(' ');

