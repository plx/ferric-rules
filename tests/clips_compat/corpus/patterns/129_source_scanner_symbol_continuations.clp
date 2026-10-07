;; Number-, sign- and dot-led lexemes run to a CLIPS delimiter as one symbol.
;; Level: boundary
;; Covers: source-scanner, explode$, symbolp, create$
(deffunction show (?source ?text)
  (bind ?scanned (explode$ ?text))
  (printout t ?text ": " ?source " " (symbolp ?source) " "
    (length$ ?scanned) " " (eq ?source (nth$ 1 ?scanned)) crlf))
(defrule probe =>
  (show 1,2 "1,2") (show 1:2 "1:2") (show 1?x "1?x") (show 1[x] "1[x]")
  (show 1' "1'") (show 1` "1`") (show 12é "12é") (show -.5λ "-.5λ")
  (show -abc "-abc") (show -abc?x "-abc?x") (show +x?y "+x?y")
  (show .foo ".foo") (show .foo? ".foo?") (show - "-") (show + "+")
  (show . ".") (show -> "->") (show -1:x "-1:x")
  (printout t (length$ (create$ 1<2)) " " (create$ 1<2) crlf)
  (printout t (length$ (create$ -a<b)) " " (create$ -a<b) crlf))
