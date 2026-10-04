(deftemplate p (multislot x (cardinality 1 2)))
(defrule bad (p (x a b c)) =>)
