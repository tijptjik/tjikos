import unittest
from bartjik_core import config, workspaces


class WorkspaceLabelTests(unittest.TestCase):
    def test_labels_preserve_ids_ownership_and_active_state(self):
        snapshot = {
            'monitors': [{'name':'DP-1','activeWorkspace':{'id':10}},
                         {'name':'DP-2','activeWorkspace':{'id':14}}],
            'workspaces': [{'id':n,'name':str(n),'monitor':'DP-1' if n<=10 else 'DP-2','windows':1}
                           for n in (1,10,11,14,20)],
        }
        labels = {'10':'X', **{str(i+10):str(i) if i<10 else 'X' for i in range(1,11)}}
        one = workspaces.view(snapshot,'DP-1',{'labels':labels,'occupiedOnly':True})
        two = workspaces.view(snapshot,'DP-2',{'labels':labels,'occupiedOnly':True})
        self.assertEqual([(r['workspace'],r['text']['value']) for r in one],[(1,'1'),(10,'X')])
        self.assertEqual([(r['id'],r['workspace'],r['text']['value']) for r in two],
                         [('11',11,'1'),('14',14,'4'),('20',20,'X')])
        self.assertTrue(one[1]['state']['active'])
        self.assertTrue(two[1]['state']['active'])
        self.assertEqual(two[1]['actions']['leftClick'],'workspace.activate')

    def test_label_validation_and_ring_appearance(self):
        config.module({'provider':'workspaces','appearance':'ring','labels':{'10':'X'}})
        for labels in ([], {'10':10}, {'10':''}, {'10':'x'*65}):
            with self.subTest(labels=labels), self.assertRaises(ValueError):
                config.module({'provider':'workspaces','labels':labels})
