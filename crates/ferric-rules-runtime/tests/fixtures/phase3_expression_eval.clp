;; Phase 3 fixture: expression evaluation
;; This fixture will be enabled when the shared expression evaluator lands.
;;
;; Expected behavior:
;; - Nested function calls in RHS are evaluated
;; - test CE evaluates boolean expressions

(defrule test-and-compute
   (value ?x)
   (test (> ?x 0))
   =>
   (assert (positive ?x))
   (assert (doubled (* ?x 2))))

(deffacts startup
   (value 5)
   (value -3))
