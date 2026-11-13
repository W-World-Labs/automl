import { normalizeStatus } from '../../lib/format';

export const entityLabel = (type: string) => {
  switch (type) {
    case 'ai_agent':
      return 'AI agent';
    case 'agent_operator':
      return 'Agent operator';
    case 'human_researcher':
      return 'Human researcher';
    default:
      return normalizeStatus(type);
  }
};

