;; Phase 3 fixture: deffunction
;; This fixture will be enabled when deffunction parsing and execution land.
;;
;; Expected behavior:
;; - Define a simple function that adds 1 to its argument
;; - Define a rule that calls the function
;; - Assert initial facts, run, verify derived facts

(deffunction add-one (?x)
   (+ ?x 1))

(defrule compute
   (value ?x)
   =>
   (assert (result (add-one ?x))))

(deffacts startup
   (value 10))
