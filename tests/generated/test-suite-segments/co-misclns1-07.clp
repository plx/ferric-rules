(deftemplate x (field q) (field r) (field s) (field t))
(defrule foo (x (t ?)) =>)
(defrule bar (x (t ?x&:(> ?x 3))) =>)
(deffacts yak (x (t abc)))
