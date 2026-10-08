;; Output queued by format or a deffunction inside nested RHS bodies precedes later direct writes.
;; Level: interaction
;; Covers: format, printout, deffunction, if, loop-for-count, while, switch, do-for-all-facts, list-focus-stack
(deftemplate item (slot n))
(deffacts items (item (n 1)) (item (n 2)))
(deffunction f () (printout t "in-f" crlf))
(deffunction g () (format t "cond%n") TRUE)
(defrule probe =>
  (if TRUE then (format t "if%n") (list-focus-stack))
  (loop-for-count (?i 2) (format t "loop %d%n" ?i) (list-focus-stack))
  (bind ?k 1)
  (while (> ?k 0) do (format t "while%n") (list-focus-stack) (bind ?k 0))
  (switch 1 (case 1 then (format t "switch%n") (list-focus-stack)))
  (do-for-all-facts ((?it item)) TRUE (format t "fact %d%n" ?it:n) (list-focus-stack))
  (if TRUE then (f) (list-focus-stack))
  (if (g) then (list-focus-stack))
  (printout t "end" crlf))
