;; call-next-method uses the next less-specific applicable method.
;; Level: basic
;; Covers: generics, next-method
(defgeneric annotate)
(defmethod annotate ((?x NUMBER)) (str-cat "number:" ?x))
(defmethod annotate ((?x INTEGER)) (str-cat "integer/" (call-next-method)))
(defrule probe => (printout t (annotate 7) crlf))
