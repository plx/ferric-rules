;; Phase 3 fixture: defglobal
;; Demonstrates global variable definition, access, and bind.

(defglobal ?*threshold* = 50)

(defrule check-threshold
   (value ?x)
   (test (> ?x ?*threshold*))
   =>
   (assert (above-threshold ?x)))

(deffacts startup
   (value 100)
   (value 25))
