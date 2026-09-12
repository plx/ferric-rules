;; A wildcard method parameter collects the remaining arguments.
;; Level: boundary
;; Covers: generics, variadic-method
(defgeneric tail-length)
(defmethod tail-length ((?head SYMBOL) $?tail) (length$ ?tail))
(defrule probe => (printout t (tail-length a) ":" (tail-length a b c) crlf))
