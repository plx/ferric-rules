;; Source numeric candidates have the same value and type as explode$ scans.
;; Level: boundary
;; Covers: source-scanner, explode$, integerp, floatp, symbolp
(deffunction show (?source ?text)
  (bind ?scanned (explode$ ?text))
  (printout t ?text ": " ?source " "
    (integerp ?source) " " (floatp ?source) " " (symbolp ?source) " "
    (length$ ?scanned) " " (eq ?source (nth$ 1 ?scanned)) crlf))
(defrule probe =>
  (show 1. "1.")
  (show 1.e3 "1.e3")
  (show .5 ".5")
  (show +.5 "+.5")
  (show -.5 "-.5")
  (show -.5e1 "-.5e1")
  (show 0x10 "0x10")
  (show 12abc "12abc")
  (show 1.abc "1.abc")
  (show 3.14.15 "3.14.15")
  (show 1-2 "1-2")
  (show 1e5x "1e5x")
  (show 1e5.5 "1e5.5")
  (show 5f "5f")
  (show 5e "5e")
  (show 1.5e "1.5e")
  (show 1e+ "1e+")
  (show 1e- "1e-"))
