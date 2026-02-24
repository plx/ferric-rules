(deftemplate foo
   (field x (type INTEGER))
   (field y (type STRING))
   (field z (type FLOAT)))
(deftemplate bar
   (field x (type INTEGER))
   (field y (type STRING))
   (field z (type FLOAT)))
(defrule bad-1 (foo (x ?x) (y ?x)) =>)
(defrule bad-2 (foo (x ?x)) (bar (y ?x)) =>)
(defrule bad-3 (foo (x ?x) (y ?y)) (bar (z ?x | ?y)) =>)
