(deftemplate foo (slot x) (slot y))
(defrule bar (foo (x 3) (x 4) (y 3)) =>)
