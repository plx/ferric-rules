; Generic and deffunction working together.
; The deffunction wraps the generic dispatch result.
(defgeneric classify)
(defmethod classify ((?x INTEGER)) "integer")
(defmethod classify ((?x FLOAT)) "float")
(defmethod classify ((?x SYMBOL)) "symbol")

(deffunction describe-value (?x) (str-cat (classify ?x) " value"))

(deffacts startup (run-it))

(defrule run-test
    (run-it)
    =>
    (printout t (describe-value 10) crlf)
    (printout t (describe-value 2.5) crlf)
    (printout t (describe-value foo) crlf))
