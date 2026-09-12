;; Dispatch checks restrictions on every regular argument.
;; Level: basic
;; Covers: generics, multiple-arguments
(defgeneric pair-kind)
(defmethod pair-kind ((?x INTEGER) (?y STRING)) integer-string)
(defmethod pair-kind ((?x STRING) (?y INTEGER)) string-integer)
(defrule probe => (printout t (pair-kind 1 "a") ":" (pair-kind "a" 1) crlf))
