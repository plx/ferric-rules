;; Issue #329: user-visible fact assertion indices.
(deftemplate item (slot value))
(deffacts seed (item (value 10)))
(deffunction through-function (?address)
  (loop-for-count (?i 1 2) do
    (printout t "function-loop:" (fact-index ?address) crlf))
  (progn$ (?piece (create$ x y))
    (printout t "function-progn:" (fact-index ?address) crlf))
  (fact-index ?address))
(defgeneric through-method)
;; The union accepts CLIPS addresses and Ferric's existing encoded integers.
(defmethod through-method ((?address FACT-ADDRESS INTEGER))
  (fact-index ?address))
(defrule probe =>
  (do-for-fact ((?f item)) TRUE
    (printout t "direct:" (fact-index ?f) crlf)
    (through-function ?f)
    (printout t "method:" (through-method ?f) crlf)
    (loop-for-count (?i 1 2) do
      (printout t "action-loop:" (fact-index ?f) crlf))
    (progn$ (?piece (create$ x y))
      (printout t "action-progn:" (fact-index ?f) crlf))))
