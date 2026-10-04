(defgeneric compare)
(defmethod compare (?x ?y) fallback)
(defmethod compare ((?x INTEGER (< ?x ?y)) (?y INTEGER)) increasing)
(defrule run => (printout t (compare 1 2) ":" (compare 2 1) crlf))
