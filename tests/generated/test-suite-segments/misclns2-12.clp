(deftemplate foo (field x (type SYMBOL)))
(defrule bar1 
  (foo (x ?x))
  =>
  (+ ?x 1))
(defrule bar2
  (foo (x ?x))
  =>
  (sym-cat ?x ?x))
(defrule bar3
  (foo (x ?x))
  =>
  (bind ?x 3)
  (sym-cat ?x ?x)
  (+ ?x 1))
(defrule bar4
  (foo (x ?x))
  =>
  (sym-cat ?x ?x)
  (+ ?x 1)
  (bind ?x 3))
