;; STRING and SYMBOL restrictions distinguish lexeme subtypes.
;; Level: basic
;; Covers: generics, lexeme-dispatch
(defgeneric classify)
(defmethod classify ((?x SYMBOL)) symbol)
(defmethod classify ((?x STRING)) string)
(defrule probe => (printout t (classify red) ":" (classify "red") crlf))
