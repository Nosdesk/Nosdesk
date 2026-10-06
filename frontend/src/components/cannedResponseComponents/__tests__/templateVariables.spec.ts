import { describe, expect, it } from 'vitest';
import {
  CANNED_RESPONSE_VARIABLES,
  findUnknownVariables,
  renderTemplate,
  unboundVariables,
} from '@nosdesk/core/services/cannedResponsesService';
import { docToString, stringToDoc } from '../templateEditor.schema';

describe('saved reply variables', () => {
  const body = 'Thanks, {{customer_first_name}}. {{agent_first_name}} ({{agent_name}}) has this.';

  it('accepts the agent spellings rule replies use', () => {
    expect(findUnknownVariables(body)).toEqual([]);
    expect(findUnknownVariables('Hi {{agnet_name}}')).toEqual(['agnet_name']);
  });

  it('fills the agent spellings from the agent on the ticket', () => {
    expect(renderTemplate(body, { customer_name: 'Jane Doe', tech_name: 'Alex Smith' })).toBe(
      'Thanks, Jane. Alex (Alex Smith) has this.',
    );
  });

  it('warns when an agent spelling would be left empty', () => {
    expect(unboundVariables(body, { customer_name: 'Jane Doe' })).toEqual([
      'agent_first_name',
      'agent_name',
    ]);
  });

  it('keeps the agent spellings as variables in the editor, without offering them as buttons', () => {
    const doc = stringToDoc(body);
    const chips: string[] = [];
    doc.descendants((node) => {
      if (node.type.name === 'variable_token') chips.push(node.attrs.name as string);
    });
    expect(chips).toEqual(['customer_first_name', 'agent_first_name', 'agent_name']);
    expect(docToString(doc)).toBe(body);
    expect(CANNED_RESPONSE_VARIABLES).not.toContain('agent_name');
  });
});
