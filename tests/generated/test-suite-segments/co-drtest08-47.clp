(defrule init
   =>
  (assert (p 1)))
(defrule crash
  (p ?X)
  (not (test (eq ?X 1)))
  (p ?Y)
  (not (and (test (neq ?Y 20))(test (neq ?Y 30))))
  =>)
