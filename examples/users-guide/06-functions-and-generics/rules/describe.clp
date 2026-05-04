(defgeneric describe)

(defmethod describe ((?x NUMBER))
    (str-cat "number(" ?x ")"))

(defmethod describe ((?x INTEGER))
    (str-cat "int/" (call-next-method)))

(defrule show
    (value ?v)
    =>
    (printout t (describe ?v) crlf))
