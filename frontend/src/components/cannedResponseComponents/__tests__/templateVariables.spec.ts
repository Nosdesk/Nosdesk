import { describe, expect, it } from 'vitest';
import {
  CANNED_RESPONSE_VARIABLES,
  findUnknownVariables,
  renderTemplate,
  renderTemplateHtml,
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

// What a saved reply puts in the composer, which holds HTML. Ticket values
// come from requesters (the email subject, the sender's name), so they land
// as text.
describe('saved reply HTML', () => {
  const vars = { ticket_title: '<img src=x> & more', customer_name: '<b>Ada</b> Lovelace' };

  it('escapes the values in a plain template', () => {
    expect(renderTemplateHtml('Re: {{ticket_title}}, {{customer_first_name}}', vars)).toBe(
      '<p>Re: &lt;img src=x&gt; &amp; more, &lt;b&gt;Ada&lt;/b&gt;</p>',
    );
  });

  it('keeps the line breaks of a plain template, and its own angle brackets as text', () => {
    expect(renderTemplateHtml('Hi {{customer_first_name}},\nThanks.\n\nIf a < b, write to <help@example.com>', vars)).toBe(
      '<p>Hi &lt;b&gt;Ada&lt;/b&gt;,<br>Thanks.</p><p>If a &lt; b, write to &lt;help@example.com&gt;</p>',
    );
  });

  it('keeps the tags of an HTML template and escapes the values', () => {
    expect(renderTemplateHtml('<p>Hi <b>{{customer_name}}</b></p><p>{{ticket_title}}</p>', vars)).toBe(
      '<p>Hi <b>&lt;b&gt;Ada&lt;/b&gt; Lovelace</b></p><p>&lt;img src=x&gt; &amp; more</p>',
    );
  });

  it('leaves unknown tokens for the agent to see', () => {
    expect(renderTemplateHtml('Hi {{custmer_name}}', vars)).toBe('<p>Hi {{custmer_name}}</p>');
  });
});
