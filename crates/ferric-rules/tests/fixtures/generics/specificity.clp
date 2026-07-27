; Method specificity: more-specific type restriction wins.
; INTEGER is more specific than NUMBER, so classify(5) => "integer".
(defgeneric classify)
(defmethod classify ((?x NUMBER)) (str-cat "number"))
(defmethod classify ((?x INTEGER)) (str-cat "integer"))

(deffacts startup (do-classify))

(defrule run-classify
    (do-classify)
    =>
    (printout t (classify 5) crlf))
