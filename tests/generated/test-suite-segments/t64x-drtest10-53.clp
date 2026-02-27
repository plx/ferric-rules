(deftemplate maze
   (multislot open-list)
   (slot goal))
(defrule test-1
   (maze (open-list)
         (goal ?g&nil))
   =>)
(defrule test-2
   (maze (open-list) 
         (goal ?g&:(eq ?g nil)))
   =>)
(defrule test-3
   (maze (open-list) 
         (goal ~nil))
   =>)
