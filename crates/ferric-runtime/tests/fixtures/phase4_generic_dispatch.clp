;; Phase 4: Generic dispatch, specificity, and call-next-method fixture.
;; Exercises defgeneric/defmethod with type-based dispatch and method chaining.

(defgeneric describe "Describe a value with type info and call-next-method")

;; Most specific: INTEGER
(defmethod describe ((?x INTEGER))
    (call-next-method)
    (* ?x 10))

;; Less specific: NUMBER (covers INTEGER + FLOAT)
(defmethod describe ((?x NUMBER))
    100)

;; Unrestricted fallback
(defmethod describe ((?x))
    0)

;; A simpler generic for testing specificity ordering
(defgeneric classify "Classify a value by type")
(defmethod classify ((?x INTEGER)) 1)
(defmethod classify ((?x FLOAT)) 2)
(defmethod classify ((?x SYMBOL)) 3)
(defmethod classify ((?x STRING)) 4)
(defmethod classify ((?x NUMBER)) 5)

(defrule test-dispatch
    (test-value ?v)
    =>
    (printout t "classify(" ?v ")=" (classify ?v) crlf))

(deffacts startup
    (test-value 42)
    (test-value 3.14)
    (test-value abc))
