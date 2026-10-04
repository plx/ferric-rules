;; A format failure preserves earlier output from its evaluated argument.
;; Level: interaction
;; Covers: format, printout, deffunction, evaluation-error
(deffunction value (?x) (printout t "argument" crlf) ?x)
(deffacts seed (input abc))
(defrule probe (input ?x)
  => (printout t "prefix " (format t "number=%d" (value ?x)) " suffix" crlf))
