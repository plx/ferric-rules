; Generic dispatch ordering: most-specific method wins.
; INTEGER > NUMBER > no restriction (wildcard).
(defgeneric classify)
(defmethod classify ((?x)) "any")
(defmethod classify ((?x NUMBER)) "number")
(defmethod classify ((?x INTEGER)) "integer")
(defmethod classify ((?x FLOAT)) "float")

(deffacts startup (run-dispatch))

(defrule run
    (run-dispatch)
    =>
    (printout t "int: " (classify 42) crlf)
    (printout t "float: " (classify 3.5) crlf)
    (printout t "sym: " (classify hello) crlf))
