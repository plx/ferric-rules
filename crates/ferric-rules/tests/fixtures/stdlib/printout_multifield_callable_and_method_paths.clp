(deffunction emit (?value) (printout t ?value crlf))
(defgeneric emit-method)
(defmethod emit-method ((?value MULTIFIELD)) (printout t ?value crlf))
(defrule exercise =>
(emit (create$ "a" "two words" "a\"b" 1.2345678901234567))
(emit-method (create$ "a" "two words" "a\\b" crlf))
(emit "plain words")
)
