import {describe,it,expect} from 'vitest';
import {labelStatus,subjectLabel,demo,questionSummary,Question} from './api';
describe('truthful learning states',()=>{it('keeps self assessment states separate',()=>{expect(labelStatus('hinted')).toBe('提示后会了');expect(labelStatus('independent')).toBe('独立做对')});it('labels demo source unambiguously',()=>{expect(demo.is_demo).toBe(true);expect(demo.source).toContain('非真实试题')});it('renders syllabus subject',()=>{expect(subjectLabel('9708')).toBe('经济');expect(subjectLabel('9702')).toBe('物理')})});
it('never presents page 14 question 30 as the text of question 31 or 32',()=>{
 const page='14 © UCLES 2023 9702/12/O/N/23 30 A fine mist of oil droplets is sprayed into air.';
 for(const ref of ['31','32']){
  const q:Question={id:ref,subject:'9702',question_no:ref,status:'legacy_unreviewed',text_scope:'whole_page_includes_adjacent_content',text:page,qp_pages:[14]};
  const summary=questionSummary(q);expect(summary).toContain('原卷第 14 页');expect(summary).toContain(`第 ${ref} 题`);expect(summary).not.toContain('oil droplets');expect(summary).not.toContain('UCLES');
 }
});
