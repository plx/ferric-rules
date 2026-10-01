;; %s formats a STRING, SYMBOL or INSTANCE-NAME; a number is an error.
;; Level: boundary
;; Covers: format
(defrule lexemes (declare (salience 10)) =>
  (printout t (format nil "%s|%s|%s|" "text" sym [inst]) crlf))
(defrule number =>
  (bind ?result (format nil "%s|" 42))
  (printout t "not reached " ?result crlf))
