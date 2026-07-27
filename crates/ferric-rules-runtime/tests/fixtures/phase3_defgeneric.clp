;; Phase 3 fixture: defgeneric + defmethod
;; Demonstrates type-based dispatch with generic functions.

(defgeneric double)

(defmethod double ((?x INTEGER)) (* ?x 2))
(defmethod double ((?x FLOAT)) (* ?x 2.0))

(defrule test-generic
   (input ?v)
   =>
   (assert (result (double ?v))))

(deffacts startup
   (input 5))
