;; STRING is more specific than LEXEME.
;; Level: boundary
;; Covers: generics, lexeme-specificity
(defgeneric classify)
(defmethod classify ((?x STRING)) string)
(defmethod classify ((?x LEXEME)) lexeme)
(defrule probe => (printout t (classify "red") ":" (classify red) crlf))
