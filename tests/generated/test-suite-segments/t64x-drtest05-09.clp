(deftemplate a (fileld one) (field two))
(defrule b                         ; DR0452
   (not (a (one first) (three second)))
   => 
   (assert (problem)))
