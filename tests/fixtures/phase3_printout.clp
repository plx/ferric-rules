;; Phase 3 fixture: printout with router
;; This fixture will be enabled when printout runtime lands.
;;
;; Expected behavior:
;; - printout writes to the specified logical router
;; - Multiple argument types are formatted correctly

(defrule greet
   (person ?name)
   =>
   (printout t "Hello, " ?name "!" crlf))

(deffacts startup
   (person Alice)
   (person Bob))
