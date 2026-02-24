(deftemplate foo (slot x (type SYMBOL)))
(defrule bar (foo (x ?x)) => (+ ?x 1))
(defrule bar (foo (x ?x)) => (assert (yak (+ ?x 1))))
